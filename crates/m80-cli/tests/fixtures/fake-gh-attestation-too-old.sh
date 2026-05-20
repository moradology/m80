#!/bin/sh
if [ "$1" = "--version" ]; then
    printf 'gh version 2.0.0\n'
    exit 0
fi
printf 'unknown command "attestation" for "gh"\n' >&2
exit 1
