# m80 v0.2.7

This release restores the public latest install path:

Run `curl -fsSL https://github.com/moradology/m80/releases/latest/download/install.sh | sudo sh`,
then `m80 run -- echo hello`.

`v0.2.7` supersedes the previous latest state: `v0.2.6` was selected by GitHub
latest but did not publish `install.sh`. The repaired stable tag is
`v0.2.7`; the released source commit is
`388d2d5aa13418e22c1144963b0f66dd3588a92b`.

Public latest URL:
https://github.com/moradology/m80/releases/latest

Pinned release URL:
https://github.com/moradology/m80/releases/tag/v0.2.7

Verification metadata:

- command: `gh release view v0.2.7 --repo moradology/m80 --json body,url,tagName`
- exit: `0`
- substrate: `public-github`
- resolved_tag: `v0.2.7`
- stdout: release body contains the v0.2.6 supersession note, public no-auth
  proof link, and protected publish proof link.
- stderr: empty

Public proof links:

- Public no-auth proof:
  https://github.com/moradology/m80/blob/main/docs/behaviors/release/release-readiness-public-access.json
- Protected publish proof:
  https://github.com/moradology/m80/blob/main/docs/behaviors/release/current-latest-repair-publish-proof.json
- Public release assets:
  https://github.com/moradology/m80/releases/tag/v0.2.7
- Protected workflow run:
  https://github.com/moradology/m80/actions/runs/26263525140

Workflow-only receipts are attached to the workflow run as Actions artifacts.
They are evidence for maintainers, not installer inputs and not public release
assets: `m80-release-publish-decision-26263525140`,
`m80-release-publication-plan-26263525140`,
`m80-release-remote-assets-26263525140`, and
`m80-release-public-access-26263525140`.

Human no-auth verification:

    tmp="$(mktemp -d)"
    base="https://github.com/moradology/m80/releases"
    GH_CONFIG_DIR="${tmp}/gh" env -u GH_TOKEN -u GITHUB_TOKEN \
      curl -fsSL "${base}/latest/download/install.sh" -o "${tmp}/latest-install.sh"
    GH_CONFIG_DIR="${tmp}/gh" env -u GH_TOKEN -u GITHUB_TOKEN \
      curl -fsSL "${base}/download/v0.2.7/install.sh" -o "${tmp}/pinned-install.sh"
    cmp "${tmp}/latest-install.sh" "${tmp}/pinned-install.sh"
    sha256sum "${tmp}/latest-install.sh"
