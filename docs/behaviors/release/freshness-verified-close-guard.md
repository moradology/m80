# Freshness verified-close guard

Public-latest freshness leaves may use hostless fake-release tests as scaffold
evidence, but those fixtures do not satisfy a verified close when the issue text
asks for public latest, public release, public asset, or public-access proof.

`scripts/verify-release-tracker-policy.py` enforces that distinction for closed
`requires-verified-close` release leaves. A matching public-proof leaf must cite
either a committed public unauthenticated proof substrate or an explicit
real-KVM proof where the leaf asks for real substrate. Local, fake, and fixture
substrates remain valid for scaffold or policy leaves that do not claim public
proof.

The policy accepts the current freshness proof substrate shape:
`network_target=public-github-release`, `auth_state=unauthenticated-public-read`,
`public_owner=moradology`, `public_repo=m80`, `fixture_source=false`, and
`github_write_apis_available=false`. It also accepts the existing public-access
receipt form with `kind=public-github` and `fixture=false`.
