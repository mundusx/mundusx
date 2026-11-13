# Link an existing contributor to Nodara

Use the updated OpenGPU desktop Settings -> Connection -> Account linking panel,
or the contributor CLI (`opengpu owner ...`, also `mundusx owner ...`).

1. In Nodara Chat Settings, enter the node ID shown by this existing contributor.
2. Generate the ten-minute link code while signed into the intended Google account.
3. Paste it into the desktop panel or run `opengpu owner link` on that device.
4. Check the account email returned by the control plane and confirm explicitly.
5. Refresh linked devices in Nodara; `opengpu owner status --json` verifies the link.

For a desktop integration, `owner inspect --code-stdin --json` previews the account,
then `owner link --code-stdin --yes --json` confirms. Pass the code on standard input,
never as a command-line argument or log entry. Invalid/expired/consumed codes fail.
Status and preview do not change ownership. Uncertain network outcomes require a
status refresh before retrying.

These commands load the existing configuration and identity without creating or
rotating a key. Requests are signed with the device's existing platform-protected
identity and use its configured control plane. Node IDs cannot replace device proof.
The `mundusx` agent wrapper forwards ownership commands to its bundled `opengpu`.
