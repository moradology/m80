mod preflight {
    mod cpu_vulnerabilities;
    mod kvm_and_os_gates;
}

#[path = "systemd/detection.rs"]
mod systemd_detection;
