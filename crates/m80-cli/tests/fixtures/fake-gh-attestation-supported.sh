#!/bin/sh
if [ "$1" = "--version" ]; then
    printf 'gh version 9.9.9\n'
    exit 0
fi
if [ "$1" = "attestation" ] && [ "$2" = "verify" ] && [ "$3" = "--help" ]; then
    printf '%s\n' '--repo --bundle --signer-workflow --cert-oidc-issuer --source-ref --source-digest --deny-self-hosted-runners --format'
    exit 0
fi
if [ "$1" = "attestation" ] && [ "$2" = "verify" ]; then
    material=${3:-}
    shift 3
    repo=
    bundle=
    signer=
    issuer=
    source_ref=
    source_digest=
    deny_self_hosted=
    format=
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --repo)
                repo=${2:-}
                shift 2
                ;;
            --bundle)
                bundle=${2:-}
                shift 2
                ;;
            --signer-workflow)
                signer=${2:-}
                shift 2
                ;;
            --cert-oidc-issuer)
                issuer=${2:-}
                shift 2
                ;;
            --source-ref)
                source_ref=${2:-}
                shift 2
                ;;
            --source-digest)
                source_digest=${2:-}
                shift 2
                ;;
            --deny-self-hosted-runners)
                deny_self_hosted=1
                shift
                ;;
            --format)
                format=${2:-}
                shift 2
                ;;
            *)
                echo "unexpected fake gh arg: $1" >&2
                exit 2
                ;;
        esac
    done
    if [ -n "${M80_FAKE_GH_ARGV:-}" ]; then
        tmp="${M80_FAKE_GH_ARGV}.$$"
        {
            printf 'attestation\nverify\n%s\n' "$material"
            printf '%s\n%s\n' "--repo" "$repo"
            printf '%s\n%s\n' "--bundle" "$bundle"
            printf '%s\n%s\n' "--signer-workflow" "$signer"
            printf '%s\n%s\n' "--cert-oidc-issuer" "$issuer"
            printf '%s\n%s\n' "--source-ref" "$source_ref"
            printf '%s\n%s\n' "--source-digest" "$source_digest"
            printf '%s\n' "--deny-self-hosted-runners"
            printf '%s\n%s\n' "--format" "$format"
        } > "$tmp"
        mv "$tmp" "$M80_FAKE_GH_ARGV"
    fi
    if [ "$repo" != "${M80_FAKE_GH_EXPECT_REPO:-moradology/m80}" ]; then
        echo "repo mismatch: $repo" >&2
        exit 1
    fi
    if [ "$(basename "$bundle")" != "${M80_FAKE_GH_EXPECT_BUNDLE_NAME:-m80-release-integrity.attestation.jsonl}" ]; then
        echo "bundle mismatch: $bundle" >&2
        exit 1
    fi
    if [ "$signer" != "${M80_FAKE_GH_EXPECT_SIGNER:-moradology/m80/.github/workflows/release-artifacts.yml}" ]; then
        echo "signer mismatch: $signer" >&2
        exit 1
    fi
    if [ "$issuer" != "${M80_FAKE_GH_EXPECT_ISSUER:-https://token.actions.githubusercontent.com}" ]; then
        echo "issuer mismatch: $issuer" >&2
        exit 1
    fi
    if [ "$source_ref" != "${M80_FAKE_GH_EXPECT_SOURCE_REF:-refs/tags/v0.0.0}" ]; then
        echo "source ref mismatch: $source_ref" >&2
        exit 1
    fi
    if [ "$source_digest" != "${M80_FAKE_GH_EXPECT_SOURCE_DIGEST:-0123456789abcdef0123456789abcdef01234567}" ]; then
        echo "source digest mismatch: $source_digest" >&2
        exit 1
    fi
    if [ "$deny_self_hosted" != "1" ]; then
        echo "missing deny self-hosted runners" >&2
        exit 1
    fi
    if [ "$format" != "json" ]; then
        echo "format mismatch: $format" >&2
        exit 1
    fi
    if [ "${M80_FAKE_GH_FAIL:-0}" = "1" ]; then
        echo "cryptographic attestation invalid" >&2
        exit 1
    fi
    if [ ! -f "$material" ]; then
        echo "material missing: $material" >&2
        exit 1
    fi
    sha=$(sha256sum "$material")
    sha=${sha%% *}
    if [ "${M80_FAKE_GH_WRONG_SUBJECT_DIGEST:-0}" = "1" ]; then
        sha=ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff
    fi
    if [ "${M80_FAKE_GH_OMIT_SUBJECT:-0}" = "1" ]; then
        printf '[{"verificationResult":{"statement":{"subject":[]}}}]\n'
    else
        name=$(basename "$material")
        printf '[{"verificationResult":{"statement":{"subject":[{"name":"%s","digest":{"sha256":"%s"}}]}}}]\n' "$name" "$sha"
    fi
    if [ -n "${M80_FAKE_GH_MARKER:-}" ]; then
        : > "$M80_FAKE_GH_MARKER"
    fi
    exit 0
fi
exit 1
