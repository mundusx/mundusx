# Qwen3.5 MLX extended context

The managed text server recognizes an opt-in `OPENGPU_MODEL_CONTEXT_TOKENS=1000000`
for Qwen3.5 models with 262144 native context. Use a current mlx-lm build supporting
Qwen3.5 and YaRN. The startup wrapper passes a full in-memory model configuration
with YaRN factor 4, original maximum 262144 and the requested maximum (at most
1010000), preserving all other config and rotary fields. Cached model files are
not changed. Lower native-context configurations keep their normal loader path.

After successful loading, `/v1/opengpu/runtime-capabilities` reports
`configuration_profile=qwen35-yarn-1m-v1` and `context_tokens=1000000`. The worker
only extends native context when that runtime proof is present. It then clamps to
available contribution memory, actual weights, sparse full-attention KV, fixed
recurrent state, retained cache, workspace and execution slots. One million is
not unconditionally advertised because a model card says it is supported.

The opt-in extended profile caps this model to one execution slot. In the unit-test estimate a 64 GiB Mac at 90% contribution
fits 1M, whereas 70% contribution or two slots reduces the limit. Do not override
an existing user contribution cap. Memory estimates are not hardware qualification:
actual 1M prefill, decoding, retrieval quality and responsiveness need a Mac test.

This managed profile is text-only (`mlx-lm`). Model-level image support still needs
a separately verified `mlx-vlm` server. Older published workers cannot activate
this feature through catalogue metadata; the updated node agent must be installed
and the managed runtime restarted with the environment above. This change does
not download a model, change an active model, replace identities or restart users'
running workers automatically.
