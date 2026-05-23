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
m80 bug-report > m80-bug-report.json
```

If a VM run directory still exists, include it in the same bundle:

```sh
m80 bug-report --vm-id <vm-id> > m80-bug-report.json
```

Do not paste secrets from your workspace, environment, or guest process output.
