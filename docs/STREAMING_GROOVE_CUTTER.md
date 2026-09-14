# Streaming groove cutter (removed)

Status: historical record. The streaming groove cutter and the paged-groove
protocol it defined were part of `src/physical/`, removed in commit
`89443b9`.

Cutting a groove and reading it back at rate *r* is a transfer function, and
the acoustic renderer expresses it directly as `cartridge_velocity_gain` and
`riaa_speed_tilt` instead of a paged cache. There is no cutter to configure
and no page protocol to satisfy; a host loads bounded PCM windows into
`ScratchAcousticDsp` instead.

See [`SCRATCH_FEEL_DECISION_LOG.md`](./SCRATCH_FEEL_DECISION_LOG.md) for the
removal.
