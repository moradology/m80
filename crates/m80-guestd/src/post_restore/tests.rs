use std::cell::{Cell, RefCell};
use std::io::{self, Cursor};
use std::path::Path;

use m80_proto::wire::generated::WirePostRestoreHookRequest;
use m80_proto::wire::WirePayload;
use m80_proto::{
    read_frame, write_frame, Envelope, HookError, HookKindWire, HookStatus, PostRestoreHookRequest,
    PostRestoreHookResponse, RawEnvelope, PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST,
};

use super::*;

#[derive(Default)]
struct FakeKernelOps {
    fail_mix: Cell<bool>,
    fail_reseed: Cell<bool>,
    fail_random: Cell<bool>,
    fail_hostname_errno: Cell<Option<i32>>,
    mixed_nonce: RefCell<Vec<[u8; 32]>>,
    reseed_calls: Cell<usize>,
    random_calls: Cell<usize>,
    hostnames: RefCell<Vec<String>>,
}

impl KernelOps for FakeKernelOps {
    fn mix_restore_nonce(&self, _urandom: &Path, restore_nonce: &[u8; 32]) -> io::Result<()> {
        if self.fail_mix.get() {
            return Err(io::Error::other("mix failed"));
        }
        self.mixed_nonce.borrow_mut().push(*restore_nonce);
        Ok(())
    }

    fn reseed_crng(&self, _urandom: &Path) -> io::Result<()> {
        if self.fail_reseed.get() {
            return Err(io::Error::from_raw_os_error(libc::ENODATA));
        }
        self.reseed_calls.set(self.reseed_calls.get() + 1);
        Ok(())
    }

    fn fill_random(&self, bytes: &mut [u8]) -> io::Result<()> {
        if self.fail_random.get() {
            return Err(io::Error::other("random failed"));
        }
        self.random_calls.set(self.random_calls.get() + 1);
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = i as u8;
        }
        Ok(())
    }

    fn set_hostname(&self, hostname: &str) -> io::Result<()> {
        if let Some(errno) = self.fail_hostname_errno.get() {
            return Err(io::Error::from_raw_os_error(errno));
        }
        self.hostnames.borrow_mut().push(hostname.to_owned());
        Ok(())
    }
}

struct Fixture {
    _temp: tempfile::TempDir,
    paths: PostRestorePaths,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        let etc = root.join("etc");
        std::fs::create_dir(&etc).expect("etc");
        Self {
            paths: PostRestorePaths {
                urandom: root.join("dev/urandom"),
                machine_id: etc.join("machine-id"),
                hostname: etc.join("hostname"),
                random_seed: root.join("var/lib/systemd/random-seed"),
            },
            _temp: temp,
        }
    }

    fn create_random_seed(&self) {
        std::fs::create_dir_all(self.paths.random_seed.parent().unwrap()).expect("seed parent");
        std::fs::write(&self.paths.random_seed, b"old-seed").expect("seed");
    }
}

fn request(hooks: Vec<HookKindWire>) -> PostRestoreHookRequest {
    PostRestoreHookRequest {
        restore_nonce: nonce(),
        hooks,
    }
}

fn nonce() -> [u8; 32] {
    let mut nonce = [0_u8; 32];
    for (i, byte) in nonce.iter_mut().enumerate() {
        *byte = (31 - i) as u8;
    }
    nonce
}

#[test]
fn handler_rejects_wrong_length_nonce_before_side_effects() {
    let ops = FakeKernelOps::default();
    let fixture = Fixture::new();
    let raw = raw_hook_request(vec![1; 31]);
    let mut out = Vec::new();

    let outcome = handle_post_restore_with_ops(raw, &mut out, &fixture.paths, &ops).unwrap();

    assert_eq!(outcome, ConnectionOutcome::Continue);
    assert!(out.is_empty());
    assert!(ops.mixed_nonce.borrow().is_empty());
    assert_eq!(ops.reseed_calls.get(), 0);
}

#[test]
fn nonce_mix_failure_returns_reseed_failed_before_hook_side_effects() {
    let ops = FakeKernelOps::default();
    ops.fail_mix.set(true);
    let fixture = Fixture::new();

    let response = run_post_restore_hooks_with_ops(
        &request(vec![HookKindWire::RegenMachineId]),
        &fixture.paths,
        &ops,
    );

    assert_eq!(response.results.len(), 1);
    assert_eq!(response.results[0].status, HookStatus::Failed);
    assert_eq!(response.results[0].error, Some(HookError::ReseedFailed));
    assert!(!fixture.paths.machine_id.exists());
}

#[test]
fn reseed_ioctl_failure_returns_reseed_failed_before_hook_side_effects() {
    let ops = FakeKernelOps::default();
    ops.fail_reseed.set(true);
    let fixture = Fixture::new();

    let response = run_post_restore_hooks_with_ops(
        &request(vec![HookKindWire::RegenMachineId]),
        &fixture.paths,
        &ops,
    );

    assert_eq!(response.results[0].error, Some(HookError::ReseedFailed));
    assert_eq!(ops.mixed_nonce.borrow().as_slice(), [nonce()]);
    assert!(!fixture.paths.machine_id.exists());
}

#[test]
fn empty_hook_list_still_mixes_nonce_and_reseeds() {
    let ops = FakeKernelOps::default();
    let fixture = Fixture::new();

    let response = run_post_restore_hooks_with_ops(&request(Vec::new()), &fixture.paths, &ops);

    assert!(response.results.is_empty());
    assert_eq!(ops.mixed_nonce.borrow().as_slice(), [nonce()]);
    assert_eq!(ops.reseed_calls.get(), 1);
}

#[test]
fn reseed_systemd_random_seed_rewrites_existing_seed() {
    let ops = FakeKernelOps::default();
    let fixture = Fixture::new();
    fixture.create_random_seed();

    let response = run_post_restore_hooks_with_ops(
        &request(vec![HookKindWire::ReseedSystemdRandomSeed]),
        &fixture.paths,
        &ops,
    );

    assert_eq!(response.results[0].status, HookStatus::Succeeded);
    assert_eq!(
        std::fs::read(&fixture.paths.random_seed).unwrap(),
        (0_u8..32).collect::<Vec<_>>()
    );
}

#[test]
fn missing_systemd_random_seed_succeeds_without_creating_parent_dirs() {
    let ops = FakeKernelOps::default();
    let fixture = Fixture::new();
    let seed_parent = fixture.paths.random_seed.parent().unwrap().to_path_buf();

    let response = run_post_restore_hooks_with_ops(
        &request(vec![HookKindWire::ReseedSystemdRandomSeed]),
        &fixture.paths,
        &ops,
    );

    assert_eq!(response.results[0].status, HookStatus::Succeeded);
    assert!(!fixture.paths.random_seed.exists());
    assert!(!seed_parent.exists());
}

#[test]
fn regen_machine_id_rewrites_tempdir_etc_machine_id() {
    let ops = FakeKernelOps::default();
    let fixture = Fixture::new();

    let response = run_post_restore_hooks_with_ops(
        &request(vec![HookKindWire::RegenMachineId]),
        &fixture.paths,
        &ops,
    );

    assert_eq!(response.results[0].status, HookStatus::Succeeded);
    assert_eq!(
        std::fs::read_to_string(&fixture.paths.machine_id).unwrap(),
        "000102030405060708090a0b0c0d0e0f\n"
    );
}

#[test]
fn set_hostname_updates_kernel_and_tempdir_etc_hostname() {
    let ops = FakeKernelOps::default();
    let fixture = Fixture::new();

    let response = run_post_restore_hooks_with_ops(
        &request(vec![HookKindWire::set_hostname("lease-1").unwrap()]),
        &fixture.paths,
        &ops,
    );

    assert_eq!(response.results[0].status, HookStatus::Succeeded);
    assert_eq!(ops.hostnames.borrow().as_slice(), ["lease-1"]);
    assert_eq!(
        std::fs::read_to_string(&fixture.paths.hostname).unwrap(),
        "lease-1\n"
    );
}

#[test]
fn hostname_syscall_failure_aborts_before_hostname_file_write() {
    let ops = FakeKernelOps::default();
    ops.fail_hostname_errno.set(Some(libc::EPERM));
    let fixture = Fixture::new();

    let response = run_post_restore_hooks_with_ops(
        &request(vec![HookKindWire::set_hostname("lease-1").unwrap()]),
        &fixture.paths,
        &ops,
    );

    assert_eq!(
        response.results[0].error,
        Some(HookError::HostnameSyscallFailed { errno: libc::EPERM })
    );
    assert!(!fixture.paths.hostname.exists());
}

#[test]
fn first_hook_failure_aborts_remaining_hooks() {
    let ops = FakeKernelOps::default();
    let fixture = Fixture::new();

    let response = run_post_restore_hooks_with_ops(
        &request(vec![
            HookKindWire::set_hostname("lease-1").unwrap(),
            HookKindWire::RegenMachineId,
        ]),
        &PostRestorePaths {
            hostname: fixture._temp.path().join("missing/etc/hostname"),
            ..fixture.paths
        },
        &ops,
    );

    assert_eq!(response.results.len(), 1);
    assert_eq!(
        response.results[0].error,
        Some(HookError::HostnameWriteFailed)
    );
}

#[test]
fn handler_decodes_request_and_writes_response() {
    let ops = FakeKernelOps::default();
    let fixture = Fixture::new();
    let env = Envelope::with_request_id(request(vec![HookKindWire::RegenMachineId]), "req-hook");
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &env).unwrap();
    let raw = m80_proto::read_raw_frame(&mut Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();

    let outcome = handle_post_restore_with_ops(raw, &mut out, &fixture.paths, &ops).unwrap();

    assert_eq!(outcome, ConnectionOutcome::Continue);
    let decoded: Envelope<PostRestoreHookResponse> = read_frame(&mut Cursor::new(out)).unwrap();
    assert_eq!(decoded.request_id.as_deref(), Some("req-hook"));
    assert_eq!(decoded.payload.results[0].status, HookStatus::Succeeded);
}

fn raw_hook_request(restore_nonce: Vec<u8>) -> RawEnvelope {
    RawEnvelope {
        version: m80_proto::PROTOCOL_VERSION,
        kind: PAYLOAD_KIND_POST_RESTORE_HOOK_REQUEST.to_owned(),
        request_id: Some("req-hook".to_owned()),
        max_duration_ms: None,
        payload: WirePayload::PostRestoreHookRequest(WirePostRestoreHookRequest {
            restore_nonce,
            hooks: Vec::new(),
        }),
    }
}
