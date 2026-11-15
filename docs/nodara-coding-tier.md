# Nodara coding capacity

The `nodara-code` OpenAI-compatible model alias is admitted by the Nodara control plane, never by a client-supplied parameter count. Standard coding requires a catalog-approved artifact with at least 14 billion **total** parameters. MoE active parameters do not replace the total count.

The node reports its served context, model identity, current available host memory, measured NVIDIA free VRAM, verified native-tool capability and execution slots. Missing GPU free-memory telemetry is unknown, not zero-cost capacity. The new `measured_free_vram_mb` is separate from the legacy configured VRAM budget. GPU devices are not assumed to pool their memory; the minimum reported device free memory is used conservatively. Non-NVIDIA GPU coding is unavailable until a measured free-memory provider is implemented.

Model approval and memory estimates belong to the control-plane catalog. Small-model text chat and ownership keys are unchanged. Updating the CLI/worker does not download a larger model or allocate more RAM. Use the control-plane model catalog to choose a suitable coding artifact; preserve contribution limits and verify runtime tools independently. A 14B text coder is not automatically a native coding agent.

Installers package the updated worker. Existing installations must update both `opengpu` and `opengpu-node-agent`; do not replace their configuration or identity keys. The 15% battery minimum is preserved.
