graph TD
    classDef open fill:#50FA7B,stroke:#333,color:#000
    classDef inprogress fill:#8BE9FD,stroke:#333,color:#000
    classDef blocked fill:#FF5555,stroke:#333,color:#000
    classDef closed fill:#6272A4,stroke:#333,color:#fff

    m80-0tf["m80-0tf<br/>Snapshot / Restore (schemas active; e..."]
    class m80-0tf open
    m80-0tf1["m80-0tf.1<br/>Manifest schemas (active)"]
    class m80-0tf1 open
    m80-0tf11["m80-0tf.1.1<br/>Reserve snapshot-manifest.json and re..."]
    class m80-0tf11 open
    m80-0tf12["m80-0tf.1.2<br/>Validate FirecrackerSnapshotManifest ..."]
    class m80-0tf12 open
    m80-0tf13["m80-0tf.1.3<br/>Validate FirecrackerRestoreMetadata s..."]
    class m80-0tf13 open
    m80-0tf2["m80-0tf.2<br/>Persistence layout (active)"]
    class m80-0tf2 open
    m80-0tf21["m80-0tf.2.1<br/>Persist snapshots under &lt;store-roo..."]
    class m80-0tf21 open
    m80-0tf22["m80-0tf.2.2<br/>Fail closed when the persistence dest..."]
    class m80-0tf22 open
    m80-0tf23["m80-0tf.2.3<br/>Restrict the first-line persistence b..."]
    class m80-0tf23 open
    m80-0tf3["m80-0tf.3<br/>Execution lane (deferred)"]
    m80-0tf31["m80-0tf.3.1<br/>Pause/quiesce the VM before snapshot ..."]
    m80-0tf32["m80-0tf.3.2<br/>Restore into a fresh vm_id and fresh ..."]
    m80-0tf4["m80-0tf.4<br/>First-line VM sizing (deferred)"]
    m80-0tf41["m80-0tf.4.1<br/>Measure snapshot timing on 1 vCPU / 1..."]
    m80-0tf42["m80-0tf.4.2<br/>Restrict the first execution slice to..."]
    m80-19i["m80-19i<br/>Concurrency, Admission & Run-Root Hyg..."]
    class m80-19i open
    m80-19i1["m80-19i.1<br/>Admission limiting"]
    class m80-19i1 open
    m80-19i11["m80-19i.1.1<br/>Gate VM creation by a tokio Semaphore..."]
    class m80-19i11 open
    m80-19i12["m80-19i.1.2<br/>Read max-concurrent-VMs from M80_FIRE..."]
    class m80-19i12 open
    m80-19i13["m80-19i.1.3<br/>Surface SandboxError::Unavailable whe..."]
    class m80-19i13 open
    m80-19i2["m80-19i.2<br/>Run-root recovery loop"]
    class m80-19i2 open
    m80-19i21["m80-19i.2.1<br/>Spawn a background recovery task once..."]
    class m80-19i21 open
    m80-19i22["m80-19i.2.2<br/>Sleep 5 seconds between recovery passes"]
    class m80-19i22 open
    m80-19i23["m80-19i.2.3<br/>Run recovery on a blocking task to av..."]
    class m80-19i23 open
    m80-19i24["m80-19i.2.4<br/>Run a one-shot startup recovery befor..."]
    class m80-19i24 open
    m80-19i3["m80-19i.3<br/>Stale-VM detection"]
    class m80-19i3 open
    m80-19i31["m80-19i.3.1<br/>Identify ownership via run-dir owners..."]
    class m80-19i31 open
    m80-19i32["m80-19i.3.2<br/>Combine lease liveness, API socket, a..."]
    class m80-19i32 open
    m80-19i33["m80-19i.3.3<br/>Preserve residue on ambiguity rather ..."]
    class m80-19i33 open
    m80-19i4["m80-19i.4<br/>Run-root layout invariants"]
    class m80-19i4 open
    m80-19i41["m80-19i.4.1<br/>Per-VM state lives under &lt;run_root..."]
    class m80-19i41 open
    m80-19i42["m80-19i.4.2<br/>Avoid cross-process collisions via sh..."]
    class m80-19i42 open
    m80-1f8["m80-1f8<br/>Observability Tail (deferred to v0.2)"]
    m80-1f81["m80-1f8.1<br/>Diagnostics event log"]
    m80-1f811["m80-1f8.1.1<br/>Write VM-lifecycle events to &lt;run_..."]
    m80-1f812["m80-1f8.1.2<br/>Cover the documented diagnostic phase..."]
    m80-1f813["m80-1f8.1.3<br/>Wrap diagnostics in Option&lt;VmDiagn..."]
    m80-1f82["m80-1f8.2<br/>Per-VM probe"]
    m80-1f821["m80-1f8.2.1<br/>Walk run_root and emit FirecrackerVmP..."]
    m80-1f822["m80-1f8.2.2<br/>Classify probe health from host-visib..."]
    m80-1f83["m80-1f8.3<br/>Health and readiness aggregation"]
    m80-1f831["m80-1f8.3.1<br/>Aggregate probe records into Firecrac..."]
    m80-1f832["m80-1f8.3.2<br/>Render health snapshot as JSON via re..."]
    m80-1f84["m80-1f8.4<br/>Prometheus scrape"]
    m80-1f841["m80-1f8.4.1<br/>Render Prometheus text via render_pro..."]
    m80-4ef["m80-4ef<br/>Errors & Failure Surfaces"]
    class m80-4ef open
    m80-4ef1["m80-4ef.1<br/>Preflight error variants"]
    class m80-4ef1 open
    m80-4ef11["m80-4ef.1.1<br/>Capture: FirecrackerBinaryNotFound pr..."]
    class m80-4ef11 open
    m80-4ef12["m80-4ef.1.2<br/>Capture: KvmUnavailable preflight var..."]
    class m80-4ef12 open
    m80-4ef13["m80-4ef.1.3<br/>Capture: UnsupportedHostPlatform pref..."]
    class m80-4ef13 open
    m80-4ef14["m80-4ef.1.4<br/>Capture: UnsupportedFirstLineVmSizing..."]
    class m80-4ef14 open
    m80-4ef15["m80-4ef.1.5<br/>Capture: PrivilegedJailerLaunchUnavai..."]
    class m80-4ef15 open
    m80-4ef2["m80-4ef.2<br/>Lifecycle error variants"]
    class m80-4ef2 open
    m80-4ef21["m80-4ef.2.1<br/>Capture: ApiSocketTimeout lifecycle v..."]
    class m80-4ef21 open
    m80-4ef22["m80-4ef.2.2<br/>Capture: ConsoleMarkerTimeout lifecyc..."]
    class m80-4ef22 open
    m80-4ef23["m80-4ef.2.3<br/>Capture: typed lifecycle failure kinds"]
    class m80-4ef23 open
    m80-4ef24["m80-4ef.2.4<br/>Capture: UnsupportedSnapshotLaunchMod..."]
    class m80-4ef24 open
    m80-4ef3["m80-4ef.3<br/>Error to CLI mapping"]
    class m80-4ef3 open
    m80-4ef31["m80-4ef.3.1<br/>Capture: error class to CLI exit-code..."]
    class m80-4ef31 open
    m80-4ef32["m80-4ef.3.2<br/>Capture: machine-readable JSON error ..."]
    class m80-4ef32 open
    m80-4ef33["m80-4ef.3.3<br/>Capture: non-error stderr is informat..."]
    class m80-4ef33 open
    m80-84x["m80-84x<br/>Cgroup v2 Limits"]
    class m80-84x open
    m80-84x1["m80-84x.1<br/>Subtree creation"]
    class m80-84x1 open
    m80-84x11["m80-84x.1.1<br/>Place per-VM cgroup under m80-firecra..."]
    class m80-84x11 open
    m80-84x12["m80-84x.1.2<br/>Enable cpu, memory, and pids in subtr..."]
    class m80-84x12 open
    m80-84x13["m80-84x.1.3<br/>Assign jailer and firecracker pids to..."]
    class m80-84x13 open
    m80-84x2["m80-84x.2<br/>Limit enforcement"]
    class m80-84x2 open
    m80-84x21["m80-84x.2.1<br/>Set cpu.max to one full CPU equivalent"]
    class m80-84x21 open
    m80-84x22["m80-84x.2.2<br/>Set memory.max to 1.5 GiB and pids.ma..."]
    class m80-84x22 open
    m80-84x23["m80-84x.2.3<br/>Gate enforcement on M80_CGROUP_MODE=u..."]
    class m80-84x23 open
    m80-84x3["m80-84x.3<br/>Failure modes"]
    class m80-84x3 open
    m80-84x31["m80-84x.3.1<br/>Refuse cgroup unified-v2 when jailer ..."]
    class m80-84x31 open
    m80-84x32["m80-84x.3.2<br/>Cleanup leaf and root cgroups idempot..."]
    class m80-84x32 open
    m80-a7g["m80-a7g<br/>Host Preflight & Discovery"]
    class m80-a7g open
    m80-a7g1["m80-a7g.1<br/>Binary discovery"]
    class m80-a7g1 open
    m80-a7g11["m80-a7g.1.1<br/>Capture: firecracker binary resolutio..."]
    class m80-a7g11 open
    m80-a7g12["m80-a7g.1.2<br/>Capture: jailer binary resolution fro..."]
    class m80-a7g12 open
    m80-a7g13["m80-a7g.1.3<br/>Capture: managed artifact directory d..."]
    class m80-a7g13 open
    m80-a7g14["m80-a7g.1.4<br/>Capture: firecracker --version probe"]
    class m80-a7g14 open
    m80-a7g15["m80-a7g.1.5<br/>Capture: expected firecracker version..."]
    class m80-a7g15 open
    m80-a7g16["m80-a7g.1.6<br/>Capture: missing firecracker binary f..."]
    class m80-a7g16 open
    m80-a7g2["m80-a7g.2<br/>KVM and OS gates"]
    class m80-a7g2 open
    m80-a7g21["m80-a7g.2.1<br/>Capture: Linux-only host platform check"]
    class m80-a7g21 open
    m80-a7g22["m80-a7g.2.2<br/>Capture: /dev/kvm presence required"]
    class m80-a7g22 open
    m80-a7g23["m80-a7g.2.3<br/>Capture: /dev/kvm writable for curren..."]
    class m80-a7g23 open
    m80-a7g24["m80-a7g.2.4<br/>Capture: effective root or passwordle..."]
    class m80-a7g24 open
    m80-a7g25["m80-a7g.2.5<br/>Capture: one-time host preflight at s..."]
    class m80-a7g25 open
    m80-a7g3["m80-a7g.3<br/>Artifact and manifest preflight"]
    class m80-a7g3 open
    m80-a7g31["m80-a7g.3.1<br/>Capture: kernel artifact auto-discove..."]
    class m80-a7g31 open
    m80-a7g32["m80-a7g.3.2<br/>Capture: rootfs path must be absolute"]
    class m80-a7g32 open
    m80-a7g33["m80-a7g.3.3<br/>Capture: manifest schema_version 1 re..."]
    class m80-a7g33 open
    m80-a7g34["m80-a7g.3.4<br/>Capture: manifest sha256 recomputatio..."]
    class m80-a7g34 open
    m80-a7g35["m80-a7g.3.5<br/>Capture: run-root creatable and writable"]
    class m80-a7g35 open
    m80-a7g36["m80-a7g.3.6<br/>Capture: storage helper binaries on PATH"]
    class m80-a7g36 open
    m80-a7g37["m80-a7g.3.7<br/>Capture: insufficient run-root capaci..."]
    class m80-a7g37 open
    m80-a7g4["m80-a7g.4<br/>Privileged-command shim"]
    class m80-a7g4 open
    m80-a7g41["m80-a7g.4.1<br/>Capture: privileged-command shim entr..."]
    class m80-a7g41 open
    m80-a7g42["m80-a7g.4.2<br/>Capture: shim direct vs sudo -n routing"]
    class m80-a7g42 open
    m80-a7g43["m80-a7g.4.3<br/>Capture: shim non-interactive contract"]
    class m80-a7g43 open
    m80-a7g44["m80-a7g.4.4<br/>Capture: privileged-shim allowed prog..."]
    class m80-a7g44 open
    m80-eb8["m80-eb8<br/>Generic Guest Exec Service"]
    class m80-eb8 open
    m80-eb81["m80-eb8.1<br/>Image contents and systemd boot"]
    class m80-eb81 open
    m80-eb811["m80-eb8.1.1<br/>Install m80 guest daemon binary at /u..."]
    class m80-eb811 open
    m80-eb812["m80-eb8.1.2<br/>Install systemd unit Type=simple Rest..."]
    class m80-eb812 open
    m80-eb813["m80-eb8.1.3<br/>Install workspace mount unit binding ..."]
    class m80-eb813 open
    m80-eb814["m80-eb8.1.4<br/>Install /etc/default env file consume..."]
    class m80-eb814 open
    m80-eb82["m80-eb8.2<br/>Vsock listener and accept loop"]
    class m80-eb82 open
    m80-eb821["m80-eb8.2.1<br/>Bind a vsock listener on a fixed port..."]
    class m80-eb821 open
    m80-eb822["m80-eb8.2.2<br/>Accept one vsock connection per reque..."]
    class m80-eb822 open
    m80-eb823["m80-eb8.2.3<br/>Sync filesystems before closing the r..."]
    class m80-eb823 open
    m80-eb83["m80-eb8.3<br/>Process orchestration"]
    class m80-eb83 open
    m80-eb831["m80-eb8.3.1<br/>Spawn child with caller-supplied argv..."]
    class m80-eb831 open
    m80-eb832["m80-eb8.3.2<br/>Capture stdout and stderr into the re..."]
    class m80-eb832 open
    m80-eb833["m80-eb8.3.3<br/>Apply caller timeout and report Timed..."]
    class m80-eb833 open
    m80-eb84["m80-eb8.4<br/>Cancellation"]
    class m80-eb84 open
    m80-eb841["m80-eb8.4.1<br/>Kill the child when the vsock connect..."]
    class m80-eb841 open
    m80-eb842["m80-eb8.4.2<br/>Flush partial output buffers before t..."]
    class m80-eb842 open
    m80-ex6["m80-ex6<br/>Acceptance Criteria"]
    class m80-ex6 closed
    m80-exy["m80-exy<br/>Networking — OutboundNat"]
    class m80-exy open
    m80-exy1["m80-exy.1<br/>Address allocation"]
    class m80-exy1 open
    m80-exy11["m80-exy.1.1<br/>Derive bridge name from sha256 of run..."]
    class m80-exy11 open
    m80-exy12["m80-exy.1.2<br/>Derive bridge CIDR from sha256 bytes ..."]
    class m80-exy12 open
    m80-exy13["m80-exy.1.3<br/>Cap bridge address space at 172.16.0...."]
    class m80-exy13 open
    m80-exy14["m80-exy.1.4<br/>Derive tap name from sha256 of (run_r..."]
    class m80-exy14 open
    m80-exy15["m80-exy.1.5<br/>Derive guest IPv4 host octet from vm_..."]
    class m80-exy15 open
    m80-exy16["m80-exy.1.6<br/>Derive guest MAC from vm_digest bytes..."]
    class m80-exy16 open
    m80-exy2["m80-exy.2<br/>Collision detection"]
    class m80-exy2 open
    m80-exy21["m80-exy.2.1<br/>Reject guest IPv4 collision with sibl..."]
    class m80-exy21 open
    m80-exy22["m80-exy.2.2<br/>Reject host route collision against /..."]
    class m80-exy22 open
    m80-exy23["m80-exy.2.3<br/>Treat collision as hard error with no..."]
    class m80-exy23 open
    m80-exy3["m80-exy.3<br/>Bridge and tap setup"]
    class m80-exy3 open
    m80-exy31["m80-exy.3.1<br/>Skip bridge realization when ownershi..."]
    class m80-exy31 open
    m80-exy32["m80-exy.3.2<br/>Persist outbound-bridge-state.json at..."]
    class m80-exy32 open
    m80-exy33["m80-exy.3.3<br/>Create TAP without ip binary then..."]
    class m80-exy33 open
    m80-exy34["m80-exy.3.4<br/>Persist per-VM network-state.json wit..."]
    class m80-exy34 open
    m80-exy35["m80-exy.3.5<br/>Assert bridge/tap setup has no..."]
    class m80-exy35 open
    m80-exy4["m80-exy.4<br/>Guest network injection"]
    class m80-exy4 open
    m80-exy41["m80-exy.4.1<br/>Discover DNS resolvers via resolvectl..."]
    class m80-exy41 open
    m80-exy42["m80-exy.4.2<br/>Admit only public IPv4 resolvers via ..."]
    class m80-exy42 open
    m80-exy43["m80-exy.4.3<br/>Reject CGN, benchmark, and reserved-r..."]
    class m80-exy43 open
    m80-exy44["m80-exy.4.4<br/>Inject 10-m80-outbound.network into p..."]
    class m80-exy44 open
    m80-exy45["m80-exy.4.5<br/>Inject 10-m80-dns.conf resolved drop-..."]
    class m80-exy45 open
    m80-exy46["m80-exy.4.6<br/>Keep guest daemon out of host-driven ..."]
    class m80-exy46 open
    m80-exy5["m80-exy.5<br/>iptables policy"]
    class m80-exy5 open
    m80-exy51["m80-exy.5.1<br/>Enable IPv4 forwarding via sysctl bef..."]
    class m80-exy51 open
    m80-exy52["m80-exy.5.2<br/>Create per-VM filter chain tfw + 12 h..."]
    class m80-exy52 open
    m80-exy53["m80-exy.5.3<br/>Accept UDP and TCP DNS to admitted re..."]
    class m80-exy53 open
    m80-exy54["m80-exy.5.4<br/>Reject all other UDP and TCP traffic ..."]
    class m80-exy54 open
    m80-exy55["m80-exy.5.5<br/>Accept bounded private IPv4 exception..."]
    class m80-exy55 open
    m80-exy56["m80-exy.5.6<br/>Reject permanent-deny CIDR list in fi..."]
    class m80-exy56 open
    m80-exy57["m80-exy.5.7<br/>Append default ACCEPT after deny list"]
    class m80-exy57 open
    m80-exy58["m80-exy.5.8<br/>Insert FORWARD entries that route gue..."]
    class m80-exy58 open
    m80-exy59["m80-exy.5.9<br/>Append NAT POSTROUTING masquerade for..."]
    class m80-exy59 open
    m80-exy6["m80-exy.6<br/>Rule tagging and teardown"]
    class m80-exy6 open
    m80-exy61["m80-exy.6.1<br/>Tag every owned iptables rule with M8..."]
    class m80-exy61 open
    m80-exy62["m80-exy.6.2<br/>Delete owned chain rules by exact com..."]
    class m80-exy62 open
    m80-exy63["m80-exy.6.3<br/>Delete per-VM filter chain only after..."]
    class m80-exy63 open
    m80-exy64["m80-exy.6.4<br/>Tolerate repeated tap deletion via if..."]
    class m80-exy64 open
    m80-exy65["m80-exy.6.5<br/>Reject foreign rules discovered in ow..."]
    class m80-exy65 open
    m80-exy7["m80-exy.7<br/>Bridge ownership and orphan recovery"]
    class m80-exy7 open
    m80-exy71["m80-exy.7.1<br/>Remove bridge only when no peer VM st..."]
    class m80-exy71 open
    m80-exy72["m80-exy.7.2<br/>Scavenge orphan bridge at startup whe..."]
    class m80-exy72 open
    m80-exy73["m80-exy.7.3<br/>Tolerate malformed or missing state f..."]
    class m80-exy73 open
    m80-exy74["m80-exy.7.4<br/>Crash mid-VM does not break new VM st..."]
    class m80-exy74 open
    m80-g3x["m80-g3x<br/>Generic Wire Protocol"]
    class m80-g3x open
    m80-g3x1["m80-g3x.1<br/>Envelope schema"]
    class m80-g3x1 open
    m80-g3x11["m80-g3x.1.1<br/>Stamp every envelope with an explicit..."]
    class m80-g3x11 open
    m80-g3x12["m80-g3x.1.2<br/>Carry an opaque request payload (prog..."]
    class m80-g3x12 open
    m80-g3x13["m80-g3x.1.3<br/>Carry response payload status, exit_c..."]
    class m80-g3x13 open
    m80-g3x2["m80-g3x.2<br/>Frame transport"]
    class m80-g3x2 open
    m80-g3x21["m80-g3x.2.1<br/>Frame envelopes as one NDJSON record ..."]
    class m80-g3x21 open
    m80-g3x22["m80-g3x.2.2<br/>Reject any frame whose post-trim leng..."]
    class m80-g3x22 open
    m80-g3x23["m80-g3x.2.3<br/>Treat malformed JSON as MalformedPayl..."]
    class m80-g3x23 open
    m80-g3x3["m80-g3x.3<br/>Version handshake"]
    class m80-g3x3 open
    m80-g3x31["m80-g3x.3.1<br/>Exchange protocol version on every co..."]
    class m80-g3x31 open
    m80-g3x32["m80-g3x.3.2<br/>Fail closed with IncompatibleVersion ..."]
    class m80-g3x32 open
    m80-mn0["m80-mn0<br/>Description"]
    class m80-mn0 closed
    m80-rrp["m80-rrp<br/>Warm-pool / Blank VM Allocator (defer..."]
    m80-rrp1["m80-rrp.1<br/>Reset evidence vocabulary"]
    m80-rrp11["m80-rrp.1.1<br/>Define BlankVmResetDecision::Reusable..."]
    m80-rrp12["m80-rrp.1.2<br/>Cover 8 reset evidence inputs (owners..."]
    m80-rrp13["m80-rrp.1.3<br/>Enumerate BlankVmResetDiscardReason f..."]
    m80-rrp2["m80-rrp.2<br/>Non-inference rule"]
    m80-rrp21["m80-rrp.2.1<br/>Forbid inferring reuse from liveness,..."]
    m80-rrp22["m80-rrp.2.2<br/>Reject any missing, stale, ambiguous,..."]
    m80-sz1["m80-sz1<br/>Image Build Pipeline"]
    class m80-sz1 open
    m80-sz11["m80-sz1.1<br/>Source artifact acquisition"]
    class m80-sz11 open
    m80-sz111["m80-sz1.1.1<br/>Resolve kernel from firecracker-ci ar..."]
    class m80-sz111 open
    m80-sz112["m80-sz1.1.2<br/>Resolve source rootfs from firecracke..."]
    class m80-sz112 open
    m80-sz113["m80-sz1.1.3<br/>Fail-closed when kernel or source roo..."]
    class m80-sz113 open
    m80-sz114["m80-sz1.1.4<br/>Probe firecracker binary for version ..."]
    class m80-sz114 open
    m80-sz115["m80-sz1.1.5<br/>Resize source rootfs ext4 image to ta..."]
    class m80-sz115 open
    m80-sz12["m80-sz1.2<br/>Chroot customization"]
    class m80-sz12 open
    m80-sz121["m80-sz1.2.1<br/>Mount output rootfs read-write via lo..."]
    class m80-sz121 open
    m80-sz122["m80-sz1.2.2<br/>Install m80 guest daemon binary into ..."]
    class m80-sz122 open
    m80-sz123["m80-sz1.2.3<br/>Install daemon systemd unit and works..."]
    class m80-sz123 open
    m80-sz13["m80-sz1.3<br/>Provenance manifest"]
    class m80-sz13 open
    m80-sz131["m80-sz1.3.1<br/>Emit provenance manifest beside the o..."]
    class m80-sz131 open
    m80-sz132["m80-sz1.3.2<br/>Pin manifest with schema_version and ..."]
    class m80-sz132 open
    m80-sz133["m80-sz1.3.3<br/>Record sha256 over kernel, source roo..."]
    class m80-sz133 open
    m80-sz134["m80-sz1.3.4<br/>Record boot_target, guest_port, and r..."]
    class m80-sz134 open
    m80-sz14["m80-sz1.4<br/>Manifest verification at boot"]
    class m80-sz14 open
    m80-sz141["m80-sz1.4.1<br/>Validate manifest schema before each ..."]
    class m80-sz141 open
    m80-sz142["m80-sz1.4.2<br/>Recompute sha256 of artifacts and ref..."]
    class m80-sz142 open
    m80-t01["m80-t01<br/>VM Boot Lifecycle"]
    class m80-t01 open
    m80-t011["m80-t01.1<br/>Run-directory layout"]
    class m80-t011 open
    m80-t0111["m80-t01.1.1<br/>Capture: per-VM run directory keyed b..."]
    class m80-t0111 open
    m80-t0112["m80-t01.1.2<br/>Capture: firecracker UDS api socket path"]
    class m80-t0112 open
    m80-t0113["m80-t01.1.3<br/>Capture: vsock host-side socket path"]
    class m80-t0113 open
    m80-t0114["m80-t01.1.4<br/>Capture: co-located rootfs clone and ..."]
    class m80-t0114 open
    m80-t0115["m80-t01.1.5<br/>Capture: run directory reaped on delete"]
    class m80-t0115 open
    m80-t012["m80-t01.2<br/>Pre-boot wiring (boot.rs)"]
    class m80-t012 open
    m80-t0121["m80-t01.2.1<br/>Capture: machine config PUT before boot"]
    class m80-t0121 open
    m80-t0122["m80-t01.2.2<br/>Capture: boot source PUT with kernel ..."]
    class m80-t0122 open
    m80-t0123["m80-t01.2.3<br/>Capture: root drive PUT for runtime r..."]
    class m80-t0123 open
    m80-t0124["m80-t01.2.4<br/>Capture: scratch workspace drive PUT"]
    class m80-t0124 open
    m80-t0125["m80-t01.2.5<br/>Capture: vsock device PUT with guest CID"]
    class m80-t0125 open
    m80-t0126["m80-t01.2.6<br/>Capture: boot identity recorded on su..."]
    class m80-t0126 open
    m80-t013["m80-t01.3<br/>UDS REST API client"]
    class m80-t013 open
    m80-t0131["m80-t01.3.1<br/>Capture: synchronous HTTP-over-UDS cl..."]
    class m80-t0131 open
    m80-t0132["m80-t01.3.2<br/>Capture: typed request configs"]
    class m80-t0132 open
    m80-t0133["m80-t01.3.3<br/>Capture: InstanceAction InstanceStart"]
    class m80-t0133 open
    m80-t0134["m80-t01.3.4<br/>Capture: InstanceAction SendCtrlAltDe..."]
    class m80-t0134 open
    m80-t0135["m80-t01.3.5<br/>Capture: typed errors for API faults"]
    class m80-t0135 open
    m80-t0136["m80-t01.3.6<br/>Capture: single-threaded blocking client"]
    class m80-t0136 open
    m80-t014["m80-t01.4<br/>Start sequence and ready detection"]
    class m80-t014 open
    m80-t0141["m80-t01.4.1<br/>Capture: serial console ready-marker ..."]
    class m80-t0141 open
    m80-t0142["m80-t01.4.2<br/>Capture: bounded ready timeout fails ..."]
    class m80-t0142 open
    m80-t0143["m80-t01.4.3<br/>Capture: post-marker vsock probe"]
    class m80-t0143 open
    m80-t0144["m80-t01.4.4<br/>Capture: vsock guest port 9001"]
    class m80-t0144 open
    m80-t0145["m80-t01.4.5<br/>Capture: full boot/stop per call"]
    class m80-t0145 open
    m80-t015["m80-t01.5<br/>Graceful stop"]
    class m80-t015 open
    m80-t0151["m80-t01.5.1<br/>Capture: x86_64 graceful via SendCtrl..."]
    class m80-t0151 open
    m80-t0152["m80-t01.5.2<br/>Capture: aarch64 forced stop"]
    class m80-t0152 open
    m80-t0153["m80-t01.5.3<br/>Capture: SIGKILL escalation after gra..."]
    class m80-t0153 open
    m80-t0154["m80-t01.5.4<br/>Capture: idempotent re-invocation of ..."]
    class m80-t0154 open
    m80-t016["m80-t01.6<br/>Delete and run-root recovery"]
    class m80-t016 open
    m80-t0161["m80-t01.6.1<br/>Capture: delete tears down sockets, t..."]
    class m80-t0161 open
    m80-t0162["m80-t01.6.2<br/>Capture: stale run-root recovery on s..."]
    class m80-t0162 open
    m80-urc["m80-urc<br/>Storage & Filesystem"]
    class m80-urc open
    m80-urc1["m80-urc.1<br/>Per-VM rootfs cloning"]
    class m80-urc1 open
    m80-urc11["m80-urc.1.1<br/>Clone managed rootfs into per-VM runt..."]
    class m80-urc11 open
    m80-urc12["m80-urc.1.2<br/>Create runtime rootfs parent director..."]
    class m80-urc12 open
    m80-urc13["m80-urc.1.3<br/>Verify managed rootfs boot identity b..."]
    class m80-urc13 open
    m80-urc14["m80-urc.1.4<br/>Surface CopyRootfs error with both pa..."]
    class m80-urc14 open
    m80-urc2["m80-urc.2<br/>Scratch workspace image"]
    class m80-urc2 open
    m80-urc21["m80-urc.2.1<br/>Build scratch ext4 image via mkfs.ext..."]
    class m80-urc21 open
    m80-urc22["m80-urc.2.2<br/>Hydrate scratch image from host works..."]
    class m80-urc22 open
    m80-urc23["m80-urc.2.3<br/>Size scratch image with padding and 4..."]
    class m80-urc23 open
    m80-urc24["m80-urc.2.4<br/>Pre-allocate scratch image file via s..."]
    class m80-urc24 open
    m80-urc25["m80-urc.2.5<br/>Reject inadmissible inodes in host wo..."]
    class m80-urc25 open
    m80-urc3["m80-urc.3<br/>Change extraction (opt-in)"]
    class m80-urc3 open
    m80-urc31["m80-urc.3.1<br/>Run change extraction only when calle..."]
    class m80-urc31 open
    m80-urc32["m80-urc.3.2<br/>Repair scratch image journal with e2f..."]
    class m80-urc32 open
    m80-urc33["m80-urc.3.3<br/>Extract changed file set with debugfs..."]
    class m80-urc33 open
    m80-urc34["m80-urc.3.4<br/>Build a writeback staging tree before..."]
    class m80-urc34 open
    m80-urc35["m80-urc.3.5<br/>Roll back to original workspace on ex..."]
    class m80-urc35 open
    m80-urc4["m80-urc.4<br/>Admissibility and atomic swap"]
    class m80-urc4 open
    m80-urc41["m80-urc.4.1<br/>Admit only regular files and director..."]
    class m80-urc41 open
    m80-urc42["m80-urc.4.2<br/>Atomically swap staged tree into host..."]
    class m80-urc42 open
    m80-v7t["m80-v7t<br/>Configuration & Discovery"]
    class m80-v7t open
    m80-v7t1["m80-v7t.1<br/>Env var schema"]
    class m80-v7t1 open
    m80-v7t11["m80-v7t.1.1<br/>Discover firecracker and jailer binar..."]
    class m80-v7t11 open
    m80-v7t12["m80-v7t.1.2<br/>Discover kernel and rootfs images via..."]
    class m80-v7t12 open
    m80-v7t13["m80-v7t.1.3<br/>Discover run-root via M80_FIRECRACKER..."]
    class m80-v7t13 open
    m80-v7t14["m80-v7t.1.4<br/>Read concurrency, jailer, and cgroup ..."]
    class m80-v7t14 open
    m80-v7t2["m80-v7t.2<br/>Config loading order"]
    class m80-v7t2 open
    m80-v7t21["m80-v7t.2.1<br/>Apply config sources in defaults → fi..."]
    class m80-v7t21 open
    m80-v7t22["m80-v7t.2.2<br/>Layer system config under user config..."]
    class m80-v7t22 open
    m80-v7t23["m80-v7t.2.3<br/>Reveal effective config via 'm80 conf..."]
    class m80-v7t23 open
    m80-v7t3["m80-v7t.3<br/>Backend construction"]
    class m80-v7t3 open
    m80-v7t31["m80-v7t.3.1<br/>Build backend once via from_discovery..."]
    class m80-v7t31 open
    m80-v7t32["m80-v7t.3.2<br/>Reuse the constructed backend across ..."]
    class m80-v7t32 open
    m80-xbn["m80-xbn<br/>Networking — NoEgress"]
    class m80-xbn open
    m80-xbn1["m80-xbn.1<br/>NoEgress configuration"]
    class m80-xbn1 open
    m80-xbn11["m80-xbn.1.1<br/>Skip bridge, tap, and NIC wiring unde..."]
    class m80-xbn11 open
    m80-xbn12["m80-xbn.1.2<br/>Provide loopback only inside the gues..."]
    class m80-xbn12 open
    m80-xbn13["m80-xbn.1.3<br/>Touch no iptables rules when NoEgress..."]
    class m80-xbn13 open
    m80-xbn14["m80-xbn.1.4<br/>Record no_egress_reason in image mani..."]
    class m80-xbn14 open
    m80-xbn2["m80-xbn.2<br/>Mode resolution"]
    class m80-xbn2 open
    m80-xbn21["m80-xbn.2.1<br/>Collapse mode resolution to a single ..."]
    class m80-xbn21 open
    m80-xbn22["m80-xbn.2.2<br/>Concentrate mode resolution behind a ..."]
    class m80-xbn22 open
    m80-xjg["m80-xjg<br/>Vsock Channel"]
    class m80-xjg open
    m80-xjg1["m80-xjg.1<br/>CID and port allocation"]
    class m80-xjg1 open
    m80-xjg11["m80-xjg.1.1<br/>Derive guest CID deterministically fr..."]
    class m80-xjg11 open
    m80-xjg12["m80-xjg.1.2<br/>Keep guest CID below reserved range t..."]
    class m80-xjg12 open
    m80-xjg13["m80-xjg.1.3<br/>Pin guest port 9001 and host UDS at &..."]
    class m80-xjg13 open
    m80-xjg2["m80-xjg.2<br/>Ready-marker probe"]
    class m80-xjg2 open
    m80-xjg21["m80-xjg.2.1<br/>Watch the serial console for the conf..."]
    class m80-xjg21 open
    m80-xjg22["m80-xjg.2.2<br/>Read ready-marker token from the root..."]
    class m80-xjg22 open
    m80-xjg23["m80-xjg.2.3<br/>Bound ready-marker wait by ready_time..."]
    class m80-xjg23 open
    m80-xjg24["m80-xjg.2.4<br/>Run probe after InstanceStart and bef..."]
    class m80-xjg24 open
    m80-xjg3["m80-xjg.3<br/>Connection lifecycle"]
    class m80-xjg3 open
    m80-xjg31["m80-xjg.3.1<br/>Open one host-to-guest vsock connecti..."]
    class m80-xjg31 open
    m80-xjg32["m80-xjg.3.2<br/>Validate the CONNECT acknowledgement ..."]
    class m80-xjg32 open
    m80-xjg33["m80-xjg.3.3<br/>Apply 5s read/write timeouts on each ..."]
    class m80-xjg33 open
    m80-ynh["m80-ynh<br/>Cleanup, Drain, Teardown"]
    class m80-ynh open
    m80-ynh1["m80-ynh.1<br/>Teardown phase order"]
    class m80-ynh1 open
    m80-ynh11["m80-ynh.1.1<br/>Drive teardown through admission_fenc..."]
    class m80-ynh11 open
    m80-ynh12["m80-ynh.1.2<br/>Fence admission before destructive te..."]
    class m80-ynh12 open
    m80-ynh13["m80-ynh.1.3<br/>Block release when forced kill is amb..."]
    class m80-ynh13 open
    m80-ynh14["m80-ynh.1.4<br/>Treat backend evidence as input, neve..."]
    class m80-ynh14 open
    m80-ynh2["m80-ynh.2<br/>Arch-sensitive stop"]
    class m80-ynh2 open
    m80-ynh21["m80-ynh.2.1<br/>Use graceful-then-force stop on x86_64"]
    class m80-ynh21 open
    m80-ynh22["m80-ynh.2.2<br/>Force-only stop on architectures with..."]
    class m80-ynh22 open
    m80-ynh23["m80-ynh.2.3<br/>Record stop disposition (Graceful, Fo..."]
    class m80-ynh23 open
    m80-ynh3["m80-ynh.3<br/>Idempotent teardown"]
    class m80-ynh3 open
    m80-ynh31["m80-ynh.3.1<br/>Repeat teardown calls without error o..."]
    class m80-ynh31 open
    m80-ynh32["m80-ynh.3.2<br/>Remove only owned runtime residue dur..."]
    class m80-ynh32 open
    m80-ynh33["m80-ynh.3.3<br/>Run the same ownership-aware cleanup ..."]
    class m80-ynh33 open
    m80-ynh4["m80-ynh.4<br/>Force-kill preservation"]
    class m80-ynh4 open
    m80-ynh41["m80-ynh.4.1<br/>Preserve run-dir for offline triage w..."]
    class m80-ynh41 open
    m80-ynh42["m80-ynh.4.2<br/>Use force_stop_and_cleanup as the las..."]
    class m80-ynh42 open
    m80-zmg["m80-zmg<br/>Jailer & Privilege Drop"]
    class m80-zmg open
    m80-zmg1["m80-zmg.1<br/>Jail root layout"]
    class m80-zmg1 open
    m80-zmg11["m80-zmg.1.1<br/>Materialize per-VM jail root under ru..."]
    class m80-zmg11 open
    m80-zmg12["m80-zmg.1.2<br/>Anchor jailer chroot base under confi..."]
    class m80-zmg12 open
    m80-zmg13["m80-zmg.1.3<br/>Persist prepared jailer plan as jaile..."]
    class m80-zmg13 open
    m80-zmg14["m80-zmg.1.4<br/>Persist jailer runtime state as jaile..."]
    class m80-zmg14 open
    m80-zmg2["m80-zmg.2<br/>Asset binding plan"]
    class m80-zmg2 open
    m80-zmg21["m80-zmg.2.1<br/>Bind kernel and firecracker binary re..."]
    class m80-zmg21 open
    m80-zmg22["m80-zmg.2.2<br/>Bind runtime rootfs and workspace scr..."]
    class m80-zmg22 open
    m80-zmg23["m80-zmg.2.3<br/>Allocate API and vsock sockets inside..."]
    class m80-zmg23 open
    m80-zmg24["m80-zmg.2.4<br/>Keep ownership marker and lease as ho..."]
    class m80-zmg24 open
    m80-zmg25["m80-zmg.2.5<br/>Compute asset binding plan before lau..."]
    class m80-zmg25 open
    m80-zmg3["m80-zmg.3<br/>Privilege drop"]
    class m80-zmg3 open
    m80-zmg31["m80-zmg.3.1<br/>Make jailed UID and GID configurable ..."]
    class m80-zmg31 open
    m80-zmg32["m80-zmg.3.2<br/>Track jailer_pid and firecracker_pid ..."]
    class m80-zmg32 open
    m80-zmg33["m80-zmg.3.3<br/>Verify launch privilege at startup once"]
    class m80-zmg33 open
    m80-zmg34["m80-zmg.3.4<br/>Surface PrivilegedJailerLaunchUnavail..."]
    class m80-zmg34 open
    m80-zmg4["m80-zmg.4<br/>Scavenge on startup"]
    class m80-zmg4 open
    m80-zmg41["m80-zmg.4.1<br/>Recover jailer state from run-dir on ..."]
    class m80-zmg41 open
    m80-zmg42["m80-zmg.4.2<br/>Identify orphan jailer processes via ..."]
    class m80-zmg42 open
    m80-zmg43["m80-zmg.4.3<br/>Preserve residue when scavenge is amb..."]
    class m80-zmg43 open

    m80-0tf1 -.-> m80-0tf
    m80-0tf11 -.-> m80-0tf1
    m80-0tf12 -.-> m80-0tf1
    m80-0tf13 -.-> m80-0tf1
    m80-0tf2 -.-> m80-0tf
    m80-0tf21 -.-> m80-0tf2
    m80-0tf22 -.-> m80-0tf2
    m80-0tf23 -.-> m80-0tf2
    m80-0tf3 -.-> m80-0tf
    m80-0tf31 -.-> m80-0tf3
    m80-0tf32 -.-> m80-0tf3
    m80-0tf4 -.-> m80-0tf
    m80-0tf41 -.-> m80-0tf4
    m80-0tf42 -.-> m80-0tf4
    m80-19i1 -.-> m80-19i
    m80-19i11 -.-> m80-19i1
    m80-19i12 -.-> m80-19i1
    m80-19i13 -.-> m80-19i1
    m80-19i2 -.-> m80-19i
    m80-19i21 -.-> m80-19i2
    m80-19i22 -.-> m80-19i2
    m80-19i23 -.-> m80-19i2
    m80-19i24 -.-> m80-19i2
    m80-19i3 -.-> m80-19i
    m80-19i31 -.-> m80-19i3
    m80-19i32 -.-> m80-19i3
    m80-19i33 -.-> m80-19i3
    m80-19i4 -.-> m80-19i
    m80-19i41 -.-> m80-19i4
    m80-19i42 -.-> m80-19i4
    m80-1f81 -.-> m80-1f8
    m80-1f811 -.-> m80-1f81
    m80-1f812 -.-> m80-1f81
    m80-1f813 -.-> m80-1f81
    m80-1f82 -.-> m80-1f8
    m80-1f821 -.-> m80-1f82
    m80-1f822 -.-> m80-1f82
    m80-1f83 -.-> m80-1f8
    m80-1f831 -.-> m80-1f83
    m80-1f832 -.-> m80-1f83
    m80-1f84 -.-> m80-1f8
    m80-1f841 -.-> m80-1f84
    m80-4ef1 -.-> m80-4ef
    m80-4ef11 -.-> m80-4ef1
    m80-4ef12 -.-> m80-4ef1
    m80-4ef13 -.-> m80-4ef1
    m80-4ef14 -.-> m80-4ef1
    m80-4ef15 -.-> m80-4ef1
    m80-4ef2 -.-> m80-4ef
    m80-4ef21 -.-> m80-4ef2
    m80-4ef22 -.-> m80-4ef2
    m80-4ef23 -.-> m80-4ef2
    m80-4ef24 -.-> m80-4ef2
    m80-4ef3 -.-> m80-4ef
    m80-4ef31 -.-> m80-4ef3
    m80-4ef32 -.-> m80-4ef3
    m80-4ef33 -.-> m80-4ef3
    m80-84x ==> m80-zmg
    m80-84x1 -.-> m80-84x
    m80-84x11 -.-> m80-84x1
    m80-84x12 -.-> m80-84x1
    m80-84x13 -.-> m80-84x1
    m80-84x2 -.-> m80-84x
    m80-84x21 -.-> m80-84x2
    m80-84x22 -.-> m80-84x2
    m80-84x23 -.-> m80-84x2
    m80-84x3 -.-> m80-84x
    m80-84x31 -.-> m80-84x3
    m80-84x32 -.-> m80-84x3
    m80-a7g1 -.-> m80-a7g
    m80-a7g11 -.-> m80-a7g1
    m80-a7g12 -.-> m80-a7g1
    m80-a7g13 -.-> m80-a7g1
    m80-a7g14 -.-> m80-a7g1
    m80-a7g15 -.-> m80-a7g1
    m80-a7g16 -.-> m80-a7g1
    m80-a7g2 -.-> m80-a7g
    m80-a7g21 -.-> m80-a7g2
    m80-a7g22 -.-> m80-a7g2
    m80-a7g23 -.-> m80-a7g2
    m80-a7g24 -.-> m80-a7g2
    m80-a7g25 -.-> m80-a7g2
    m80-a7g3 -.-> m80-a7g
    m80-a7g31 -.-> m80-a7g3
    m80-a7g32 -.-> m80-a7g3
    m80-a7g33 -.-> m80-a7g3
    m80-a7g34 -.-> m80-a7g3
    m80-a7g35 -.-> m80-a7g3
    m80-a7g36 -.-> m80-a7g3
    m80-a7g37 -.-> m80-a7g3
    m80-a7g4 -.-> m80-a7g
    m80-a7g41 -.-> m80-a7g4
    m80-a7g42 -.-> m80-a7g4
    m80-a7g43 -.-> m80-a7g4
    m80-a7g44 -.-> m80-a7g4
    m80-eb8 ==> m80-t01
    m80-eb81 -.-> m80-eb8
    m80-eb811 -.-> m80-eb81
    m80-eb812 -.-> m80-eb81
    m80-eb813 -.-> m80-eb81
    m80-eb814 -.-> m80-eb81
    m80-eb82 -.-> m80-eb8
    m80-eb821 -.-> m80-eb82
    m80-eb822 -.-> m80-eb82
    m80-eb823 -.-> m80-eb82
    m80-eb83 -.-> m80-eb8
    m80-eb831 -.-> m80-eb83
    m80-eb832 -.-> m80-eb83
    m80-eb833 -.-> m80-eb83
    m80-eb84 -.-> m80-eb8
    m80-eb841 -.-> m80-eb84
    m80-eb842 -.-> m80-eb84
    m80-exy1 -.-> m80-exy
    m80-exy11 -.-> m80-exy1
    m80-exy12 -.-> m80-exy1
    m80-exy13 -.-> m80-exy1
    m80-exy14 -.-> m80-exy1
    m80-exy15 -.-> m80-exy1
    m80-exy16 -.-> m80-exy1
    m80-exy2 -.-> m80-exy
    m80-exy2 ==> m80-exy1
    m80-exy21 -.-> m80-exy2
    m80-exy22 -.-> m80-exy2
    m80-exy23 -.-> m80-exy2
    m80-exy3 -.-> m80-exy
    m80-exy3 ==> m80-exy2
    m80-exy31 -.-> m80-exy3
    m80-exy32 -.-> m80-exy3
    m80-exy33 -.-> m80-exy3
    m80-exy34 -.-> m80-exy3
    m80-exy35 -.-> m80-exy3
    m80-exy4 -.-> m80-exy
    m80-exy4 ==> m80-exy3
    m80-exy41 -.-> m80-exy4
    m80-exy42 -.-> m80-exy4
    m80-exy43 -.-> m80-exy4
    m80-exy44 -.-> m80-exy4
    m80-exy45 -.-> m80-exy4
    m80-exy46 -.-> m80-exy4
    m80-exy5 -.-> m80-exy
    m80-exy5 ==> m80-exy3
    m80-exy51 -.-> m80-exy5
    m80-exy52 -.-> m80-exy5
    m80-exy53 -.-> m80-exy5
    m80-exy54 -.-> m80-exy5
    m80-exy55 -.-> m80-exy5
    m80-exy56 -.-> m80-exy5
    m80-exy57 -.-> m80-exy5
    m80-exy58 -.-> m80-exy5
    m80-exy59 -.-> m80-exy5
    m80-exy6 -.-> m80-exy
    m80-exy6 ==> m80-exy5
    m80-exy61 -.-> m80-exy6
    m80-exy62 -.-> m80-exy6
    m80-exy63 -.-> m80-exy6
    m80-exy64 -.-> m80-exy6
    m80-exy65 -.-> m80-exy6
    m80-exy7 -.-> m80-exy
    m80-exy7 ==> m80-exy3
    m80-exy71 -.-> m80-exy7
    m80-exy72 -.-> m80-exy7
    m80-exy73 -.-> m80-exy7
    m80-exy74 -.-> m80-exy7
    m80-g3x1 -.-> m80-g3x
    m80-g3x11 -.-> m80-g3x1
    m80-g3x12 -.-> m80-g3x1
    m80-g3x13 -.-> m80-g3x1
    m80-g3x2 -.-> m80-g3x
    m80-g3x21 -.-> m80-g3x2
    m80-g3x22 -.-> m80-g3x2
    m80-g3x23 -.-> m80-g3x2
    m80-g3x3 -.-> m80-g3x
    m80-g3x31 -.-> m80-g3x3
    m80-g3x32 -.-> m80-g3x3
    m80-rrp1 -.-> m80-rrp
    m80-rrp11 -.-> m80-rrp1
    m80-rrp12 -.-> m80-rrp1
    m80-rrp13 -.-> m80-rrp1
    m80-rrp2 -.-> m80-rrp
    m80-rrp21 -.-> m80-rrp2
    m80-rrp22 -.-> m80-rrp2
    m80-sz1 ==> m80-a7g
    m80-sz11 -.-> m80-sz1
    m80-sz111 -.-> m80-sz11
    m80-sz112 -.-> m80-sz11
    m80-sz113 -.-> m80-sz11
    m80-sz114 -.-> m80-sz11
    m80-sz115 -.-> m80-sz11
    m80-sz12 -.-> m80-sz1
    m80-sz121 -.-> m80-sz12
    m80-sz122 -.-> m80-sz12
    m80-sz123 -.-> m80-sz12
    m80-sz13 -.-> m80-sz1
    m80-sz131 -.-> m80-sz13
    m80-sz132 -.-> m80-sz13
    m80-sz133 -.-> m80-sz13
    m80-sz134 -.-> m80-sz13
    m80-sz14 -.-> m80-sz1
    m80-sz141 -.-> m80-sz14
    m80-sz142 -.-> m80-sz14
    m80-t01 ==> m80-a7g
    m80-t011 -.-> m80-t01
    m80-t0111 -.-> m80-t011
    m80-t0112 -.-> m80-t011
    m80-t0113 -.-> m80-t011
    m80-t0114 -.-> m80-t011
    m80-t0115 -.-> m80-t011
    m80-t012 -.-> m80-t01
    m80-t0121 -.-> m80-t012
    m80-t0122 -.-> m80-t012
    m80-t0123 -.-> m80-t012
    m80-t0124 -.-> m80-t012
    m80-t0125 -.-> m80-t012
    m80-t0126 -.-> m80-t012
    m80-t013 -.-> m80-t01
    m80-t0131 -.-> m80-t013
    m80-t0132 -.-> m80-t013
    m80-t0133 -.-> m80-t013
    m80-t0134 -.-> m80-t013
    m80-t0135 -.-> m80-t013
    m80-t0136 -.-> m80-t013
    m80-t014 -.-> m80-t01
    m80-t0141 -.-> m80-t014
    m80-t0142 -.-> m80-t014
    m80-t0143 -.-> m80-t014
    m80-t0144 -.-> m80-t014
    m80-t0145 -.-> m80-t014
    m80-t015 -.-> m80-t01
    m80-t0151 -.-> m80-t015
    m80-t0152 -.-> m80-t015
    m80-t0153 -.-> m80-t015
    m80-t0154 -.-> m80-t015
    m80-t016 -.-> m80-t01
    m80-t0161 -.-> m80-t016
    m80-t0162 -.-> m80-t016
    m80-urc ==> m80-t01
    m80-urc1 -.-> m80-urc
    m80-urc11 -.-> m80-urc1
    m80-urc12 -.-> m80-urc1
    m80-urc13 -.-> m80-urc1
    m80-urc14 -.-> m80-urc1
    m80-urc2 -.-> m80-urc
    m80-urc21 -.-> m80-urc2
    m80-urc22 -.-> m80-urc2
    m80-urc23 -.-> m80-urc2
    m80-urc24 -.-> m80-urc2
    m80-urc25 -.-> m80-urc2
    m80-urc3 -.-> m80-urc
    m80-urc31 -.-> m80-urc3
    m80-urc32 -.-> m80-urc3
    m80-urc33 -.-> m80-urc3
    m80-urc34 -.-> m80-urc3
    m80-urc35 -.-> m80-urc3
    m80-urc4 -.-> m80-urc
    m80-urc41 -.-> m80-urc4
    m80-urc42 -.-> m80-urc4
    m80-v7t1 -.-> m80-v7t
    m80-v7t11 -.-> m80-v7t1
    m80-v7t12 -.-> m80-v7t1
    m80-v7t13 -.-> m80-v7t1
    m80-v7t14 -.-> m80-v7t1
    m80-v7t2 -.-> m80-v7t
    m80-v7t21 -.-> m80-v7t2
    m80-v7t22 -.-> m80-v7t2
    m80-v7t23 -.-> m80-v7t2
    m80-v7t3 -.-> m80-v7t
    m80-v7t31 -.-> m80-v7t3
    m80-v7t32 -.-> m80-v7t3
    m80-xbn1 -.-> m80-xbn
    m80-xbn11 -.-> m80-xbn1
    m80-xbn12 -.-> m80-xbn1
    m80-xbn13 -.-> m80-xbn1
    m80-xbn14 -.-> m80-xbn1
    m80-xbn2 -.-> m80-xbn
    m80-xbn21 -.-> m80-xbn2
    m80-xbn22 -.-> m80-xbn2
    m80-xjg ==> m80-t01
    m80-xjg1 -.-> m80-xjg
    m80-xjg11 -.-> m80-xjg1
    m80-xjg12 -.-> m80-xjg1
    m80-xjg13 -.-> m80-xjg1
    m80-xjg2 -.-> m80-xjg
    m80-xjg21 -.-> m80-xjg2
    m80-xjg22 -.-> m80-xjg2
    m80-xjg23 -.-> m80-xjg2
    m80-xjg24 -.-> m80-xjg2
    m80-xjg3 -.-> m80-xjg
    m80-xjg31 -.-> m80-xjg3
    m80-xjg32 -.-> m80-xjg3
    m80-xjg33 -.-> m80-xjg3
    m80-ynh ==> m80-t01
    m80-ynh1 -.-> m80-ynh
    m80-ynh11 -.-> m80-ynh1
    m80-ynh12 -.-> m80-ynh1
    m80-ynh13 -.-> m80-ynh1
    m80-ynh14 -.-> m80-ynh1
    m80-ynh2 -.-> m80-ynh
    m80-ynh21 -.-> m80-ynh2
    m80-ynh22 -.-> m80-ynh2
    m80-ynh23 -.-> m80-ynh2
    m80-ynh3 -.-> m80-ynh
    m80-ynh31 -.-> m80-ynh3
    m80-ynh32 -.-> m80-ynh3
    m80-ynh33 -.-> m80-ynh3
    m80-ynh4 -.-> m80-ynh
    m80-ynh41 -.-> m80-ynh4
    m80-ynh42 -.-> m80-ynh4
    m80-zmg1 -.-> m80-zmg
    m80-zmg11 -.-> m80-zmg1
    m80-zmg12 -.-> m80-zmg1
    m80-zmg13 -.-> m80-zmg1
    m80-zmg14 -.-> m80-zmg1
    m80-zmg2 -.-> m80-zmg
    m80-zmg21 -.-> m80-zmg2
    m80-zmg22 -.-> m80-zmg2
    m80-zmg23 -.-> m80-zmg2
    m80-zmg24 -.-> m80-zmg2
    m80-zmg25 -.-> m80-zmg2
    m80-zmg3 -.-> m80-zmg
    m80-zmg31 -.-> m80-zmg3
    m80-zmg32 -.-> m80-zmg3
    m80-zmg33 -.-> m80-zmg3
    m80-zmg34 -.-> m80-zmg3
    m80-zmg4 -.-> m80-zmg
    m80-zmg41 -.-> m80-zmg4
    m80-zmg42 -.-> m80-zmg4
    m80-zmg43 -.-> m80-zmg4
