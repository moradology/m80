# Conntrack Exhaustion Guardrails

OutboundNat uses Linux conntrack for NAT state, but `nf_conntrack_max` is
host-global. A single guest must not be able to fill that table and deny
service to sibling VMs or host NAT users.

Preflight reads `/proc/sys/net/netfilter/nf_conntrack_max` and requires at
least `2 * M80_MAX_CONCURRENT_VMS * 1000` entries. `M80_MAX_CONCURRENT_VMS`
defaults to 8 and is also the backend admission semaphore size, so the sizing
input is the same value that controls concurrent VM count.

Each OutboundNat VM also receives a TAP-scoped TCP SYN connlimit rule:
`-i <tap> -s <guest_ipv4>/32 -p tcp --syn -m connlimit --connlimit-above 256 --connlimit-mask 32 -j REJECT`.
The rule is comment-tagged and removed during normal OutboundNat cleanup with
the rest of the VM-owned FORWARD rules.
