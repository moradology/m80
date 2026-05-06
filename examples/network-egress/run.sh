#!/bin/sh
set -eu

m80 run --egress outbound -- sh -c '/bin/busybox wget -qO- http://example.com | /bin/busybox head -n 1'
