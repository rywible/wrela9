use wrela_compiler::{
    ArchitectureProfile, Cancellation, CompilationOutcome, CompilationRequest, Compiler,
    CompilerInstallation, DiagnosticValue, InspectSelection, ProjectFile, ProjectSnapshot, Root,
};

fn image_request() -> CompilationRequest {
    CompilationRequest::new(
        ProjectSnapshot::new(vec![ProjectFile::new(
            "src/image.wr",
            b"@image\nfn build() -> Image:\n    return Image.new()\n",
        )]),
        Root::Image,
    )
}

#[test]
fn unsupported_public_profile_selection_rejects_canonically() {
    fn compile() -> CompilationOutcome {
        Compiler::open(CompilerInstallation::layer1())
            .expect("distribution opens")
            .compile(
                image_request().with_architecture_profile(ArchitectureProfile::X86_64),
                &Cancellation::new(),
            )
    }

    let first = compile();
    let repeated = compile();
    assert_eq!(first, repeated);
    let CompilationOutcome::Rejected(rejected) = first else {
        panic!("the later x86-64 profile is unsupported in the current version");
    };
    assert_eq!(rejected.diagnostics().len(), 1);
    let diagnostic = &rejected.diagnostics()[0];
    assert_eq!(diagnostic.code(), "architecture.unsupported_profile");
    assert_eq!(diagnostic.primary().path(), "src/image.wr");
    assert_eq!(diagnostic.primary().start(), 0);
    assert_eq!(diagnostic.primary().end(), 0);
    assert_eq!(
        diagnostic.typed_parameters(),
        &[("profile".into(), DiagnosticValue::Text("x86_64".into()))]
    );
}

#[test]
fn current_aarch64_selection_authenticates_the_planning_contract() {
    let compiler = Compiler::open(CompilerInstallation::layer1()).expect("distribution opens");
    let outcome = compiler.compile(
        image_request()
            .with_architecture_profile(ArchitectureProfile::CurrentAarch64)
            .with_inspection(InspectSelection::all()),
        &Cancellation::new(),
    );
    let CompilationOutcome::Accepted(accepted) = outcome else {
        panic!("the current AArch64 planning contract must authenticate");
    };
    let contract = accepted
        .inspection()
        .architecture_planning_contract()
        .expect("selected contract is inspected");

    assert_eq!(
        accepted.inspection().distribution_version(),
        "wrela9-layer2-architecture-v1"
    );
    assert_eq!(contract.profile(), ArchitectureProfile::CurrentAarch64);
    assert_eq!(contract.contract_schema_version(), 1);
    assert_eq!(contract.contract_version(), 1);
    assert_eq!(
        contract.distribution_input_receipt(),
        186_242_940_987_556_221_601_299_593_776_526_439_257
    );
    assert_eq!(
        contract.identity(),
        82_368_861_742_248_649_671_883_035_618_079_068_922
    );
    assert_eq!(
        contract.fingerprint(),
        25_050_859_001_709_894_488_791_727_024_515_932_124
    );
    assert_eq!(contract.symbolic_core_count(), 4);
    assert_eq!(contract.page_quantum_bytes(), 4096);
    assert_eq!(contract.minimum_ram_bytes(), 128 * 1024 * 1024);
    assert_eq!(contract.maximum_ram_bytes(), 2 * 1024 * 1024 * 1024);
}
