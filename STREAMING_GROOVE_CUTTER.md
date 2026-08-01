# Streaming groove cutter

## Purpose

The streaming groove cutter converts sequential PCM into the canonical 192 kHz groove format.

Use this cutter during loading, decoding, or authoring.

Do not use this cutter on an audio render thread.

The cutter does not keep the complete source or groove in memory.

## Terms

A **core** is the unique output range that belongs to one page.

A **halo** is the overlapping range that supports tracing and spatial filtering at a page boundary.

A **raw page chunk** contains a core and its available halos.

A **snapshot** contains the bounded state that controls all subsequent output.

## Input contract

Provide the total PCM frame count before you create the cutter.

Provide one mono channel or two stereo channels.

Use a finite source rate greater than zero and not greater than 192 kHz.

Submit each chunk with its absolute first source frame.

Submit chunks without gaps or overlaps.

The cutter validates a complete chunk before it changes its state.

The cutter rejects nonfinite PCM before it changes its state.

The cutter rejects more input after `finish()` succeeds.

## Numerical contract

The cutter uses the same processing sequence as `GrooveAsset::cut_from_pcm`.

The sequence contains Catmull-Rom resampling, cutter high-pass filtering, RIAA filtering, and the 45/45 matrix.

The sequence then uses trapezoidal velocity integration to calculate displacement.

The cutter keeps enough look-ahead for the four-sample interpolation kernel.

At the declared end, the cutter clamps look-ahead to the final source sample.

This rule produces the same tail samples and output frame count as the monolithic cut.

Partition boundaries do not change displacement samples, the cut report, or the groove content identity.

The content identity covers the base lateral and vertical displacement streams.

The spatial pyramid is derived data and is not part of the source groove identity.

## Memory bounds

The cutter keeps no more than four source history frames after a successful call.

Four frames contain the complete Catmull-Rom interpolation support.

The clearance history limit is `ceil(groove_rate * 60 / rpm) + 1` frames.

The cutter keeps no clearance history when the complete program is shorter than one revolution.

The pending page limit is `core + 2 * storage_halo` frames.

The storage halo contains the declared tracing halo and the spatial filter margin.

Use the bound methods on `StreamingGrooveCutterConfig` to calculate these limits.

The page callback receives each completed raw chunk synchronously.

The caller controls the storage lifetime of emitted chunks.

Use `push_chunk_until_page()` when the caller can retain only one page.

This method consumes zero or more source frames and returns at most one page.

Store the returned page before you submit the unconsumed source frames again.

The method can consume zero frames when accepted input can produce another page.

## Finalization and pages

Call `finish()` only after you submit the declared source length.

Drain all remaining pages before you call `finish()`.

Use empty channel slices with `push_chunk_until_page()` to drain accepted input.

Finalization returns the exact frame count, cut report, and canonical content identity.

Create `PhysicalGrooveMetadata` from the finalization result.

Convert each raw chunk with metadata that has the same total length and storage halo.

`PhysicalGroovePage` then creates and validates its derived spatial pyramid.

`PagedGrooveAsset` validates core continuity, halo overlap, and page identity.

## Snapshots

`StreamingGrooveCutterSnapshot` has an explicit format version.

The snapshot contains filter, integration, hashing, clearance, source, and pending page state.

Serialize the snapshot with Serde when a decoder must pause or move between hosts.

Restore a snapshot only with its original cutter configuration.

The restore operation validates the snapshot before it changes the destination cutter.

A restored snapshot can emit a page that the original cutter emitted after that snapshot.

Discard such later pages before you replay input from an earlier snapshot.

## Integration rule

Native Swift code must call this implementation through the `record-player` crate boundary.

Do not make a second Swift implementation of the cutter pipeline.

## Evidence limit

The tests prove numerical equivalence with the current canonical monolithic implementation.

The tests do not prove empirical agreement with a calibrated cutter, cartridge, stylus, or record surface.

Do not use this implementation alone as evidence for an accuracy or calibration claim.
