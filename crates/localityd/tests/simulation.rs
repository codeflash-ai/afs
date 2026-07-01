use locality_core::simulation_harness::{
    SimulationConfig, SimulationHarness, SimulationOutcome, SimulationProfile,
};

#[test]
fn simulation_smoke_exercises_core_reliability_harness() {
    let outcome = SimulationHarness::run(SimulationConfig {
        seed: 0xdae0_0001,
        steps: 96,
        profile: SimulationProfile::Crashy,
    })
    .expect("daemon-facing simulation smoke should converge");

    assert_eq!(outcome, SimulationOutcome::Converged);
}

#[test]
#[ignore = "nightly reliability profile; run with LOCALITY_SIMULATION_PROFILE=nightly"]
fn simulation_nightly_reliability_profile() {
    let seeds = SimulationProfile::Nightly.default_seeds();

    for seed in seeds {
        let outcome = SimulationHarness::run(SimulationConfig {
            seed,
            steps: 512,
            profile: SimulationProfile::Nightly,
        })
        .unwrap_or_else(|error| panic!("simulation seed {seed:#x} failed: {error}"));

        assert_eq!(outcome, SimulationOutcome::Converged, "seed {seed:#x}");
    }
}
