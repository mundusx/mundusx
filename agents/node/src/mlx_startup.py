"""Report actual completed weight evaluation; never generate qualification tokens."""
import json
import os
from pathlib import Path
import threading
import re

PREFIX = "MUNDUSX_LOAD_PROGRESS:"
BATCH_BYTES = 64 * 1024 * 1024


class RequiredToolPrefix:
    """Per-request opening only: never supplies a tool name or arguments."""
    def __init__(self, tokens):
        if not tokens:
            raise ValueError("Required tool prefix must not be empty")
        self.tokens = tuple(tokens)
        self.start = None

    def next_token(self, input_length):
        if self.start is None:
            self.start = input_length
        offset = input_length - self.start
        if offset < 0:
            raise ValueError("Required tool token history moved backwards")
        return self.tokens[offset] if offset < len(self.tokens) else None


class RegisteredToolOpening:
    """Constrain only the opening and registered function name, never values."""
    def __init__(self, sequences):
        if not sequences or any(not sequence for sequence in sequences):
            raise ValueError("Registered tool openings must not be empty")
        self.sequences = tuple(tuple(sequence) for sequence in sequences)
        self.start = None

    def next_tokens(self, history):
        if self.start is None:
            self.start = len(history)
        generated = tuple(history[self.start:])
        candidates = [sequence for sequence in self.sequences if sequence[:len(generated)] == generated]
        if not candidates:
            raise ValueError("Generated tool opening left the registered tool set")
        if any(len(sequence) == len(generated) for sequence in candidates):
            return None
        return sorted({sequence[len(generated)] for sequence in candidates})


def install_required_tool_opening(server, mx):
    original_tokenize = server.ResponseGenerator._tokenize
    original_processors = server._make_logits_processors

    def tokenize(self, tokenizer, request, args):
        options = args.chat_template_kwargs or {}
        required = options.get("opengpu_require_tool_call") is True
        if required:
            if request.request_type != "chat" or not request.tools or not tokenizer.has_tool_calling:
                raise ValueError("Required tool opening needs a tool-capable chat request")
            if not tokenizer.tool_parser.__module__.endswith("qwen3_coder"):
                raise ValueError("Required tool opening supports the Qwen3-Coder parser only")
            names = [tool.get("function", {}).get("name") for tool in request.tools]
            if any(not isinstance(name, str) or not name for name in names):
                raise ValueError("Required tool opening needs registered function names")
            args._opengpu_required_openings = tuple(tuple(tokenizer.encode("<tool_call>\n<function=" + name + ">\n", add_special_tokens=False)) for name in names)
        args.chat_template_kwargs = {key: value for key, value in options.items() if key != "opengpu_require_tool_call"}
        return original_tokenize(self, tokenizer, request, args)

    def processors(args):
        result = list(original_processors(args))
        openings = getattr(args, "_opengpu_required_openings", None)
        if openings:
            prefix = RegisteredToolOpening(openings)
            finished = False

            def force_opening(history, logits):
                nonlocal finished
                if finished:
                    return logits
                tokens = prefix.next_tokens(history.tolist())
                if tokens is None:
                    finished = True
                    return logits
                allowed = mx.any(mx.arange(logits.shape[-1])[:, None] == mx.array(tokens)[None, :], axis=1)
                return mx.where(allowed, logits, mx.full_like(logits, -float("inf")))

            result.append(force_opening)
        return result

    server.ResponseGenerator._tokenize = tokenize
    server._make_logits_processors = processors


def install_capabilities_endpoint(server):
    """Let a new agent distinguish a patched running server from an old one."""
    original_get = server.APIHandler.do_GET

    def get(self):
        if self.path != "/v1/opengpu/runtime-capabilities":
            return original_get(self)
        payload = json.dumps({"required_tool_opening": True, "version": 3}).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    server.APIHandler.do_GET = get


def complete_qwen_call_parser(parser):
    """Keep complete calls when Qwen adds prose after the closing function."""
    def parse(text, tools=None):
        matches = list(re.finditer(r"<function=[^<>\s]+>[\s\S]*?</function>", text))
        if len(matches) != 1:
            raise ValueError("Expected one complete Qwen function block")
        match = matches[0]
        block = match.group(0)
        if block.count("<function=") != 1 or block.count("<parameter=") != block.count("</parameter>"):
            raise ValueError("Nested or incomplete Qwen function or parameter blocks")
        remainder = text[:match.start()] + text[match.end():]
        if re.search(r"</?(?:function|parameter|tool_call)(?:[=>\s]|$)", remainder):
            raise ValueError("Incomplete or ambiguous extra Qwen tool markup")
        # The original schema-aware parser still converts every argument.
        # Never synthesize missing values, closing tags, or additional calls.
        return parser(block, tools)
    return parse


def install_complete_qwen_parser(server):
    original_init = server.ToolCallFormatter.__init__

    def initialize(self, tool_parser, tools, streaming=False):
        if getattr(tool_parser, "__module__", "").endswith("qwen3_coder"):
            tool_parser = complete_qwen_call_parser(tool_parser)
        original_init(self, tool_parser, tools, streaming)

    server.ToolCallFormatter.__init__ = initialize


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
    import mlx_lm.server as server
    install_required_tool_opening(server, mx)
    install_complete_qwen_parser(server)
    install_capabilities_endpoint(server)
    server.main()


if __name__ == "__main__":
    main()
