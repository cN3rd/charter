# Pitfalls

Append-only. Each entry: trigger, wrong behavior, right behavior. Charter's local rules for this
vendored skill and the mistakes it has caused here.

- Trigger: the red step in Rust, for a function that doesn't exist yet. Wrong: counting the
  compile error as the failing test; the skill wants a failure, not an error. Right: add the
  signature with a stub that returns a wrong value, watch `cargo test` fail on the assertion, then
  implement.
- Trigger: testing a float that went through division or rounding, such as a BPM round trip.
  Wrong: asserting its exact last bit, which pins an accident of the arithmetic that a correct
  change can break. Right: assert what the code promises: the value after the rounding the format
  applies, or a stated tolerance.
