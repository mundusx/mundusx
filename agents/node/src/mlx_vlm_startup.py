"""Managed Qwen3.5 vision server with verified in-memory context configuration."""
import json
import os
from pathlib import Path

from mlx_startup import qwen35_long_context_config, verify_qwen35_loaded_context


def vision_runtime_proof(model, processor, requested, model_name):
    if getattr(model, "vision_tower", None) is None or getattr(processor, "image_processor", None) is None:
        raise ValueError("The selected runtime did not load its vision encoder and processor")
    proof = {"engine": "mlx-vlm", "version": 12, "supports_vision": True, "model": model_name}
    if requested > 262144:
        verify_qwen35_loaded_context(model, requested)
        proof.update(context_tokens=requested, configuration_profile="qwen35-yarn-1m-v1")
    else:
        proof["context_tokens"] = requested or model.config.text_config.max_position_embeddings
    return proof


def native_tools_runtime_proof(model, processor, runtime_version):
    """Only claim the pinned Qwen3.5 runtime whose tool protocol was validated."""
    tokenizer = getattr(processor, "tokenizer", processor)
    template = getattr(processor, "chat_template", None) or getattr(tokenizer, "chat_template", "")
    return (runtime_version == "0.8.1"
            and getattr(model.config, "model_type", None) == "qwen3_5_moe"
            and isinstance(template, str)
            and "tools" in template and "<tool_call>" in template)


def main():
    import mlx_vlm.utils as utils

    requested = int(os.environ.get("OPENGPU_MODEL_CONTEXT_TOKENS", "0"))
    ready = Path(os.environ["OPENGPU_MLX_READY_FILE"])
    original_config = utils.load_config
    original_load = utils.load
    import sys
    model_name = sys.argv[sys.argv.index("--model") + 1]
    proof = {"engine": "mlx-vlm", "version": 12, "supports_vision": False, "model": model_name}

    def configured_load(path, **kwargs):
        config = original_config(path, **kwargs)
        override = qwen35_long_context_config(config, requested)
        return override if override is not None else config

    def measured_load(*args, **kwargs):
        try:
            model, processor = original_load(*args, **kwargs)
            if kwargs.get("lazy", False):
                raise ValueError("Managed vision readiness requires evaluated weights")
            proof.update(vision_runtime_proof(model, processor, requested, model_name))
            from importlib.metadata import version
            proof["native_tool_calls_v1"] = native_tools_runtime_proof(model, processor, version("mlx-vlm"))
            ready.write_text("ready", encoding="utf-8")
            return model, processor
        except Exception as error:
            proof["supports_vision"] = False
            ready.with_suffix(".error").write_text(str(error)[:4096], encoding="utf-8")
            raise

    utils.load_config = configured_load
    utils.load = measured_load
    os.environ["APC_ENABLED"] = "0"
    os.environ["APC_DISK_ENABLED"] = "0"
    os.environ["MLX_VLM_VISION_CACHE_SIZE"] = "0"
    os.environ["MLX_VLM_MAX_NUM_SEQS"] = "1"
    from mlx_vlm.server import app

    @app.get("/v1/opengpu/runtime-capabilities")
    def capabilities():
        return dict(proof)

    # mlx-lm's caller passes a cache-count option that mlx-vlm does not use.
    import sys
    if "--prompt-cache-size" in sys.argv:
        index = sys.argv.index("--prompt-cache-size")
        del sys.argv[index:index + 2]
    from mlx_vlm.server.cli import main as serve
    serve()


if __name__ == "__main__":
    main()
