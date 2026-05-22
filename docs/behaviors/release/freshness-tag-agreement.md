# Freshness Tag Agreement

The hostless freshness verifier fails closed unless every public install
identity resolves to the same stable release. The agreement set is:

- GitHub latest release metadata after stable-channel eligibility filtering;
- the tag selected by `scripts/stable_latest_bootstrap.py`;
- the concrete tag embedded in the pinned `install.sh` URL;
- the public bundle metadata sidecar, through `release_tag`, `m80_version`, and
  `package_version`.

`scripts/release_freshness.py` records these values in the proof
`tag_agreement` section. Success records the latest tag, stable bootstrap tag,
pinned install URL tag, bundle metadata release tag, bundle metadata
`m80_version`, and bundle metadata `package_version`. A mismatch is classified
as `stale-latest`, names the disagreed pair, prints expected and observed
values, and includes the pinned install command for the selected stable tag.

Draft releases, prereleases, and non-`vMAJOR.MINOR.PATCH` tags remain
ineligible before agreement is checked. That keeps the latest channel pointed
at a stable public release and prevents a latest URL, pinned URL, or bundle
metadata sidecar from silently proving a different release than the one a user
would install.

