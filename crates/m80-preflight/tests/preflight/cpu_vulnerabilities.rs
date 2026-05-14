use m80_preflight::PreflightError;

#[test]
fn cpu_vulnerability_error_names_status_file_and_kernel_text() {
    let err = PreflightError::CpuVulnerabilityDetected {
        id: "l1tf".to_owned(),
        detail: "Vulnerable: SMT vulnerable".to_owned(),
    };

    let rendered = err.to_string();

    assert!(rendered.contains("l1tf"));
    assert!(rendered.contains("Vulnerable: SMT vulnerable"));
    assert!(err.hint().contains("M80_SKIP_CHECK_VULNERABILITIES=1"));
}
