# Decisions: host identity on the cw-app audit line (ORB-29040)

## Windows audit lines carry no host-derived identity

- **Deviates from:** `STD-05@1 §R1`, which requires identity and provenance to derive from
  state the host controls. Holds on Unix; not met on Windows.
- **Decision:** On Unix, each audit line records `euid`, the effective user ID of the process
  that wrote it, stamped at write time (`crates/cw-app/src/audit/host.rs`). On Windows
  `euid` is `null` on every line. The line's only identity there is `self_reported_actor`,
  which is a claim under `STD-05@1 §R1` and `§R2`.
- **Why:** The kit has no Windows account lookup today. A value for it would need a Windows
  API dependency that `cw-app` does not take. A recorded `null` is more honest than a
  fabricated or copied identity.
- **Consequence:** A Windows line says the host identity is unknown. Readers must treat
  `euid: null` as "not recorded", never as "no user".
- **Revisit:** When `cw-app` takes a Windows API dependency, stamp the account identifier on
  Windows and remove this deviation.
