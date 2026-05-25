# MMDS Non-Use

Firecracker's MicroVM Metadata Service (MMDS) serves caller-provided JSON to a
guest through link-local networking. m80 does not expose MMDS in v0.x.

## Decision

m80 is generic VM mechanics. It has no agent identity, workspace identity,
cluster metadata, credential authority, or tool-call semantic model. Those are
adapter concerns, not m80 concerns. Passing them through an in-guest link-local
HTTP service would make m80 own product metadata semantics that belong above
the VM boundary.

Adapters should pass explicit metadata through existing m80 request surfaces:

- environment variables on an exec request,
- workspace files intentionally placed by the caller,
- typed wire/file operations,
- adapter-owned side channels outside this repo.

## Link-Local Block

m80's outbound NAT policy rejects `169.254.0.0/16` in every per-VM filter chain.
That IMDS block is structural and independent of whether Firecracker MMDS is
configured. See [`docs/behaviors/network/imds-block.md`](../behaviors/network/imds-block.md).

Because m80 does not configure MMDS, there is no in-guest link-local metadata
endpoint to document, secure, or authorize.

## Revisit Rule

Adding MMDS requires a new bead and an adapter-boundary update. The bead must
name the metadata owner, authorization model, guest-visible schema, and why
existing exec env/files/wire operations are insufficient.
