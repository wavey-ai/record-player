# Physical player WASM API

This API gives an AudioWorklet direct access to the canonical physical renderer.

The generated JavaScript module does not require a C header.

`PhysicalHostRenderer` owns the deck, groove source, cartridge, phono stage, resampler, and output buffer.

## Create the renderer

Create one renderer with a supported host sample rate and an explicit output level.

```js
const renderer = new PhysicalHostRenderer(sampleRate, {
  voltsPerFullScale: 10,
});
```

`voltsPerFullScale` gives the phono voltage that maps to digital full scale.

The value must be finite, positive, and no more than 1,000 volts.

The options object can also contain a complete serialized `PhysicalProfile` value.

The constructor validates a supplied profile. Omit `profile` to use the built-in seed profile.

The renderer divides each phono sample by `voltsPerFullScale`.

It then clips the host sample to the range from -1 through 1.

Use the host output telemetry getters to detect clipping and retain the unclipped voltage peak.

- `outputVoltsPerFullScale()` returns the configured boundary.
- `hostOutputPeakUnclippedLeftV()` returns the last block's left voltage peak.
- `hostOutputPeakUnclippedRightV()` returns the last block's right voltage peak.
- `hostOutputClippedLeftSamples()` returns the last block's left clipping count.
- `hostOutputClippedRightSamples()` returns the last block's right clipping count.
- `hostOutputTotalClippedLeftSamples()` returns the cumulative left count.
- `hostOutputTotalClippedRightSamples()` returns the cumulative right count.

The clipping-count getters return JavaScript `bigint` values.

Use `PhysicalHostRenderer.supportsOutputSampleRate(rate)` before construction when the host rate is not known.

The supported rates are 44,100, 48,000, 88,200, 96,000, 176,400, and 192,000 Hz.

## Use the output buffer

`outputBufferPtr()` and `outputBufferLen()` identify one Rust-owned interleaved stereo buffer.

The buffer allocation does not move during render-only operation.

An allocating lifecycle call can grow WASM memory. Memory growth replaces the JavaScript `ArrayBuffer`.

Compare `memoryViewGeneration()` after each allocating lifecycle call.

Create a new `Float32Array` when the generation changes.

```js
let viewGeneration = renderer.memoryViewGeneration();
let output = new Float32Array(
  wasm.memory.buffer,
  renderer.outputBufferPtr(),
  renderer.outputBufferLen(),
);

function refreshOutputView() {
  const currentGeneration = renderer.memoryViewGeneration();
  if (currentGeneration === viewGeneration) return;
  viewGeneration = currentGeneration;
  output = new Float32Array(
    wasm.memory.buffer,
    renderer.outputBufferPtr(),
    renderer.outputBufferLen(),
  );
}
```

The following calls can change the generation:

- `loadInterleavedPcm()`
- `loadRealtimePagedCache()`
- `beginPagedCache()`
- `beginPagedCacheUpdate()`
- `preparePagedPage()`
- `commitPreparedPagedPage()`
- `insertPagedPage()`
- `publishPagedCache()`
- `buildPagedPrefetchPlan()`
- `snapshot()`
- `restore()`

## Render audio

Call `render(frameCount)` from the AudioWorklet render callback.

The successful path does not allocate or serialize telemetry.

Read `outputFrameCount() * 2` samples after `PhysicalRenderStatus.Ok`.

The call returns `PhysicalRenderStatus.BlockTooLarge` when the block exceeds `maximumRenderFrames()`.

A failed render sets `outputFrameCount()` to zero. It preserves renderer state and previous output-buffer contents.

## Schedule complete controls

All controls use absolute 192 kHz frame numbers.

Use `internalFrameAfterHostFrames(offset)` to calculate an exact future frame boundary.

JavaScript represents frame and sequence arguments as `bigint` values.

Call `enqueueTimedControl()` with every host control field. Do not send partial state changes.

Do not send stylus reaction torque. The physical player calculates and owns this feedback value.

Use `PhysicalMotorMode` values for the motor mode argument.

The control call does not allocate. It returns one `PhysicalControlStatus` value.

Pointer gesture code must use `PhysicalScratchGestureMapper`. Do not calculate scratch motion in JavaScript.

Use `beginScalar()`, `updateScalar()`, and `finishScalar()` in the live input path.

These calls return `PhysicalScratchGestureStatus`. They do not serialize a JavaScript object.

Read the `scalar*` result getters after a successful call.

Merge every returned hand field into the next complete timed control.

The object-returning gesture methods are for diagnostics and lifecycle tools only.

## Load complete PCM

`loadInterleavedPcm(samples, channelCount, sourceSampleRateHz)` accepts one or two interleaved channels.

This call cuts the complete programme into the canonical spatial groove.

This call allocates and can take significant time. Do not call it from the audio render callback.

Use `sourceIdentityPresent()`, `sourceIdentityVersion()`, and `sourceIdentityByte(index)` to read the canonical SHA-256 identity.

Use `setGrooveFramePosition(position)` to seek in the 192 kHz groove coordinate.

Use `resetTransport()` to reset platter and record rates and angles.

Use `unloadGroove()` to remove either a complete or paged source.

## Load paged groove data

Paged metadata and pages use the canonical serialized Rust schemas.

Stop rendering before you create, change, validate, or publish a paged cache.

These lifecycle operations allocate and can take significant time.

An AudioWorklet `MessagePort` handler runs on the rendering thread.

The handler does not make page lifecycle work safe during playback.

Prepare the first cache before playback starts. Suspend playback before you replace pages with this API.

Call `beginPagedCache(metadata, maximumPages, maximumResidentBytes)` to start a new cache.

Pass zero for both limits to use the canonical defaults.

For each page, call `preparePagedPage(storedFrameCount)` to allocate bounded typed storage.

The maximum stored length is available from `maximumPreparedPagedPageFrames()`.

Refresh all WASM views after `preparePagedPage()`.

Use `preparedPagedPageLateralPtr()` and `preparedPagedPageVerticalPtr()` to create two `Float32Array` views.

Copy one channel of displacement data into each view.

Call `commitPreparedPagedPage(coreStart, coreEnd, storedStart, storedEnd)` to validate and insert the page.

All four range values are `bigint` values. Each range is half-open and uses absolute 192 kHz frames.

The commit builds the spatial pyramid. It consumes the prepared data, including after a page construction failure.

Call `preparePagedPage()` again before you retry a failed commit.

`insertPagedPage(page)` is an allocating diagnostic import path for a serialized canonical page.

Call `publishPagedCache()` to freeze the staged cache.

Call `loadPublishedPagedCache()` to use the cache generation from its metadata.

Use this load call only for the first cache or an intentional source reset.

Call `beginPagedCacheUpdate()` to copy the published cache into a new producer.

Use `removePagedPageContaining(frame)` and the typed staging calls to change the staged cache.

Publish the updated cache while rendering is stopped.

Call `refreshLoadedPagedCache()` to replace pages without resetting physical state.

The refresh requires identical metadata, content, generation, and source descriptors.

The refresh does not make page creation or publication safe on the rendering thread.

Call `buildPagedPrefetchPlan(horizon, turnOffsets)` after a render or control change.

Build this plan while rendering is stopped because the current planner allocates.

This lifecycle call stores a bounded plan in fixed facade storage.

Use `pagedPrefetchRangeCount()` and the scalar range getters to read the plan.

Each range is half-open and uses absolute 192 kHz groove frames.

The cached prefetch getters do not allocate or serialize JSON.

The getters are safe in the render path only after lifecycle planning is complete.

Use the staged and published scalar getters to inspect generation, size, limits, frame count, and tracing halo.

`PhysicalRenderStatus.PageMiss` identifies a missing page or a stale generation.

Read `lastPageMissKind()` after this status.

The remaining page-miss getters return the frame and generation identifiers without allocation.

## Stream pages into the fixed cache

Use the fixed cache for page changes during playback.

Call `loadRealtimePagedCache()` while rendering is stopped.

This call allocates all base, pyramid, and page-slot storage.

Pass zero for every cache limit to use the canonical defaults.

Use `beginRealtimePagedPagePrecomputed()` for the production path.

Pass the page identity as eight big-endian 32-bit SHA-256 words.

Use the canonical page ranges from `PhysicalGroovePage`.

Read the returned slot and sequence with the last-ticket getters.

Use `readRealtimePagedSpatialLevelLayout()` to verify each worker level layout.

Reserve one base or pyramid channel chunk.

Read its reservation sequence, pointer, and length.

Create a `Float32Array` on the Rust-owned destination.

Copy the worker chunk into that view synchronously.

Call `commitRealtimePagedReservedChunk()` before you return control to the browser.

Call `cancelRealtimePagedReservedChunk()` when you cannot complete the write.

Do not retain the pointer after a commit or cancel.

A failed nonfinite-value commit keeps the reservation active.

Rewrite the destination or cancel the reservation after this failure.

Call `finishRealtimePagedPageIngestion()` after both base channels and all four levels are complete.

Call `advanceRealtimePagedPage()` with a bounded work budget until the phase is `Ready`.

Call `publishRealtimePagedPage()` to make the validated page visible in one state change.

The cache validates the page identity and all overlapping seam samples before publication.

An incomplete, stale, corrupt, or rejected page remains invisible to the renderer.

Use `evictRealtimePagedPageContaining()` to reuse storage from a published page.

Use `discardRealtimePagedPage()` only for a staged or rejected page.

These calls do not allocate after `loadRealtimePagedCache()` succeeds.

`beginRealtimePagedPage()` builds the pyramid in the renderer instance.

Do not use that raw path for sustained rapid scratching.

The 65-tap construction work cannot sustain a 20-times source-frame demand in a practical callback budget.

## Cut long PCM in a worker

Use `StreamingGrooveCutter` in a Web Worker for a long decoded source.

Do not create this cutter in an AudioWorklet.

Use a separate WASM instance so cutter allocations cannot replace the renderer memory buffer.

Create a complete `StreamingGrooveCutterConfig` value before construction.

Use `createSeedConfig()` when the built-in physical profile is sufficient.

Call `seedMinimumTracingHaloFrames()` with the source rate and declared source length.

Pass the returned halo to `createSeedConfig()`.

A smaller halo cannot trace the seed stylus over the complete record.

Call `pushPlanarF32()` with planar `Float32Array` channels.

Call `pushPlanarS16()` with planar `Int16Array` channels.

The signed conversion divides each sample by 32,768.

Pass an empty right channel for mono input.

Each push returns `StreamingGrooveCutterStatus`.

Read `lastConsumedSourceFrameCount()` after each push.

Advance the absolute input frame by this count.

Resubmit the unconsumed input suffix after you store a returned page.

`StreamingGrooveCutterStatus.PageReady` means that one page is ready.

Call `takeEmittedPage()` before you push more input.

The cutter retains no more than one emitted page.

The page object owns its Rust allocation.

Use its range getters and `storedFrameCount()` to read the page descriptor.

Use `lateralDisplacementPtr()` and `verticalDisplacementPtr()` to create zero-copy views.

Copy these views to durable worker storage before the next WASM allocation.

Call `free()` on the page object after the copy.

`toValue()` is an optional serialized path. It copies the complete page.

Call `finish()` after the cutter consumes the declared source length.

If finish returns `PageReady`, store that page and call `finish()` again.

`StreamingGrooveCutterStatus.Finalized` means that all pages and final data are ready.

Use `finalMetadata(generation)`, `finalReport()`, and the final identity getters after finalization.

The final metadata identity is necessary before a raw page can become a canonical page.

Call `materialize(metadata)` when one raw page remains in the same worker instance.

Use `PhysicalGroovePageMaterializer` to reimport a stored raw page without JSON.

Create the materializer after finalization with `finalMetadata(generation)`.

Set its maximum stored frame count to the largest raw page.

Call `prepareRawPage()` with the raw page format, ranges, total length, and halo.

Write both raw channels into the two fixed staging pointers.

Call `materializePreparedPage()` to build the canonical page and its spatial pyramid.

This worker call allocates. Refresh its WASM views after the memory generation changes.

Call `takeMaterializedPage()` to get the completed `PhysicalGroovePage`.

The page exposes the base channels and the 2-times, 4-times, 8-times, and 16-times levels.

It also exposes the canonical page identity as eight big-endian 32-bit words.

Copy or transfer each exported channel before you free the page.

Materialize only one page at a time to keep worker memory bounded.

The progress and bound getters use scalar values.

The cutter snapshot includes bounded continuation state and one optional pending page.

The restore operation rejects an invalid snapshot before it changes the cutter.

A restored checkpoint can emit pages that you stored after that checkpoint.

Remove those later pages before you replay source input.

## Read clocks and telemetry

Use `currentInternalFrame()` and `renderedHostFrames()` to read both clock domains.

Use `latencyInternalFrames()` or `latencySeconds()` to read output-filter latency.

Telemetry getters return scalar values. They do not create JavaScript objects.

The getters cover transport, contact, cartridge, phono, overload, and radial tracking state.

## Save deterministic state

`snapshot()` serializes all mutable renderer state. The call allocates.

`restore(snapshot)` validates and restores that state. The same source identity must already be loaded.

Refresh cached WASM views after either call.
