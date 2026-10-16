"""Report actual completed weight evaluation; never generate qualification tokens."""
import json
import os
from pathlib import Path
import runpy
import threading

PREFIX = "MUNDUSX_LOAD_PROGRESS:"
BATCH_BYTES = 64 * 1024 * 1024


def bounded_prompt_cache(cache_class, max_bytes):
    """Cap retained prefixes on every insertion, including non-batched requests."""
    if max_bytes < 0:
        raise ValueError("Prompt cache byte budget must not be negative")

    class BoundedPromptCache(cache_class):
        def __init__(self, max_size=2, **kwargs):
            kwargs["max_bytes"] = min(kwargs.get("max_bytes", max_bytes), max_bytes)
            super().__init__(max_size=max_size, **kwargs)

    return BoundedPromptCache


def leaves(value):
    if isinstance(value, dict):
        for item in value.values():
            yield from leaves(item)
    elif isinstance(value, (list, tuple)):
        for item in value:
            yield from leaves(item)
    elif isinstance(getattr(value, "nbytes", None), int):
        yield value


def evaluate_weights(evaluate, parameters, report, batch_bytes=BATCH_BYTES):
    arrays = list({id(array): array for array in leaves(parameters)}.values())
    total = sum(array.nbytes for array in arrays)
    if not total:
        return evaluate(parameters)
    report(0, total)
    batch, size, completed = [], 0, 0
    result = None
    for array in arrays:
        batch.append(array)
        size += array.nbytes
        if size >= batch_bytes:
            result = evaluate(*batch)
            completed += size
            report(completed, total)
            batch, size = [], 0
    if batch:
        result = evaluate(*batch)
        completed += size
        report(completed, total)
    return result


def main():
    import mlx.core as mx
    import mlx_lm.utils as utils
    import mlx_lm.models.cache as cache_module

    cache_module.LRUPromptCache = bounded_prompt_cache(
        cache_module.LRUPromptCache,
        int(os.environ["OPENGPU_MLX_PROMPT_CACHE_BYTES"]),
    )

    state = threading.local()
    original_eval = mx.eval
    original_load = utils.load
    ready = Path(os.environ["OPENGPU_MLX_READY_FILE"])
    startup_complete = False

    def report(completed, total):
        print(PREFIX + json.dumps({"completed_bytes": completed, "total_bytes": total}), flush=True)

    def measured_eval(*args, **kwargs):
        if getattr(state, "loading", False) and len(args) == 1 and isinstance(args[0], dict) and not kwargs:
            return evaluate_weights(original_eval, args[0], report)
        return original_eval(*args, **kwargs)

    def measured_load(*args, **kwargs):
        nonlocal startup_complete
        previous = getattr(state, "loading", False)
        state.loading = not startup_complete
        try:
            result = original_load(*args, **kwargs)
            if not startup_complete and not kwargs.get("lazy", False):
                ready.write_text("ready", encoding="utf-8")
                startup_complete = True
            return result
        except Exception as error:
            if not startup_complete:
                ready.with_suffix(".error").write_text(str(error)[:4096], encoding="utf-8")
            raise
        finally:
            state.loading = previous

    mx.eval = measured_eval
    utils.load = measured_load
    runpy.run_module("mlx_lm.server", run_name="__main__")


if __name__ == "__main__":
    main()
