---
name: Bug report
about: Report an m80 failure with host and VM diagnostics
title: ""
labels: bug
assignees: ""
---

## Summary

What failed?

## Reproduction

```sh
# command you ran
```

## Diagnostic bundle

Attach the output of:

```sh
m80 --json env > m80-env.json
```

If a VM run directory still exists, also attach:

```sh
m80 --json logs <vm-id> > m80-logs.json
```

Do not paste secrets from your workspace, environment, or guest process output.
