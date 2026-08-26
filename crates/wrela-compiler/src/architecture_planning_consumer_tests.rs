use crate::Cancellation;
use crate::architecture_planning::ArchitectureProfile;
use crate::compiler::CompilerInstallation;
use crate::distribution::CompilerDistribution;

#[test]
fn sibling_consumers_reach_each_planning_fact_through_purpose_owned_views() {
    let distribution =
        CompilerDistribution::seal(CompilerInstallation::layer1()).expect("distribution seals");
    let contract = distribution
        .architecture_planning()
        .authenticate(ArchitectureProfile::CurrentAarch64, &Cancellation::new())
        .expect("current contract authenticates");

    let admission = contract.for_admission();
    assert_eq!(admission.capabilities().len(), 9);
    assert_eq!(admission.capacity().maximum_requirements(), 65_536);
    assert_eq!(admission.device_slots().len(), 8);
    assert_eq!(admission.binding_slots().len(), 8);
    assert_eq!(admission.interrupts().route_slots(), 4);
    assert_eq!(admission.dma().maximum_in_flight(), 1024);

    let service = contract.for_service_analysis();
    assert_eq!(service.cores().len(), 4);
    assert_eq!(service.costs().schema_version(), 1);
    assert_eq!(service.costs().maximum_cycle_units(), 1_000_000);
    assert_eq!(service.costs().maximum_cancellation_delay_units(), 250_000);

    let layout = contract.for_logical_layout();
    assert_eq!(layout.capacity().minimum_ram_bytes(), 134_217_728);
    assert_eq!(layout.alignment().region_bytes(), 4096);
    assert_eq!(layout.page().quantum_bytes(), 4096);
    assert_eq!(layout.guards().normal_stack_before_pages(), 1);
    assert_eq!(layout.reservations().len(), 6);
    assert_eq!(layout.envelopes().len(), 8);
    assert_eq!(layout.regions().len(), 6);
    assert_eq!(layout.dma().required_alignment(), 4096);
}
