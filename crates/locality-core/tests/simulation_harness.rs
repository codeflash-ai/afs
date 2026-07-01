use locality_core::simulation_harness::{
    SimulationConfig, SimulationHarness, SimulationOutcome, SimulationProfile,
};

#[test]
fn simulation_smoke_replays_seeded_sequence_to_convergence() {
    let outcome = SimulationHarness::run(SimulationConfig {
        seed: 0x5eed_0001,
        steps: 64,
        profile: SimulationProfile::Smoke,
    })
    .expect("simulation should complete without invariant violations");

    assert_eq!(outcome, SimulationOutcome::Converged);
}

#[test]
fn simulation_replays_interrupted_pushes_without_losing_content() {
    let outcome = SimulationHarness::run(SimulationConfig {
        seed: 0x5eed_f411,
        steps: 96,
        profile: SimulationProfile::Crashy,
    })
    .expect("interrupted push simulation should remain recoverable");

    assert_eq!(outcome, SimulationOutcome::Converged);
}

#[test]
#[ignore = "nightly reliability profile; run with LOCALITY_SIMULATION_PROFILE=nightly"]
fn simulation_nightly_profile_runs_many_seeded_sequences() {
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
