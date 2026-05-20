#!/bin/sh
if [ "$1" = "--version" ]; then
    printf 'gh version 9.9.9\n'
    exit 0
fi
if [ "$1" = "attestation" ] && [ "$2" = "verify" ] && [ "$3" = "--help" ]; then
    printf '%s\n' '--repo --bundle --signer-workflow --cert-oidc-issuer --source-ref --source-digest --deny-self-hosted-runners --format'
    exit 0
fi
exit 1
