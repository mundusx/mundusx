# Saved model context limits

CLI and node-agent `config.json` retain a `model_context_limits` map keyed by
the exact model ID. A value of 0 explicitly selects Automatic; a positive
value is the requested context ceiling. Missing entries preserve the legacy
launch-environment behaviour. The desktop editor accepts Automatic or integers
from 1024 to the catalogue maximum (bounded at 16777216 for unknown maxima).

Settings → Contribution → Allowable context edits one model at a time. Save
requires contribution to be stopped and preserves other settings, credentials
and model choices. The next Start uses the saved value without requiring a
terminal environment variable. The CLI session settings editor shares this
map and drains work before restarting for a context change.

Saved settings win over `OPENGPU_MODEL_CONTEXT_TOKENS`, including Automatic.
The managed MLX child receives its own context environment; process-global
environment values are not changed. Requested context feeds both slot selection
and the conservative context estimator. Known model/runtime and memory limits
continue to clamp the report, and unknown architectures can only be capped,
never expanded from the saved number.

For Qwen3.5, selecting 1000000 activates the opt-in YaRN profile. Switching
between native and extended profiles reloads only a recorded, ownership-verified
managed MLX process. Unowned external servers are not stopped automatically.
Actual 1M inference and vision still require separate hardware qualification.
The CLI, node agent and desktop must all be updated together so older writers
do not discard the new field. No running Mac installation is changed by this
source update.
