# Production Crash Investigation

## Purpose

This document records production player failures and the evidence for each conclusion.

Do not use an inference as a confirmed cause. Keep rejected causes and incomplete evidence.

## Incident IOS-2026-08-02-1556

### Report identity

- The TestFlight feedback comment is `15:56`.
- The incident identifier is `B693B01D-48AC-435E-A1BC-A3137A7352EF`.
- The application version is `1.0 (22)`.
- The device is an iPhone 13 mini.
- The operating system is iOS 26.5.2.
- The crash occurred at 2026-08-02 15:56:32 Europe/London.
- The application launched at 15:36:28.

### Confirmed evidence

The process received `SIGABRT` on audio render thread 9. The crash occurred while `AVAudioSourceNode` requested program audio.

The callback used the native stereo planar renderer. The Swift call site is `NativeRecordAudioPipeline.swift:842`.

The archived dSYM UUID matches the application image UUID. Both UUIDs are `315AB821-85A7-370C-AFC1-E10D49A42256`.

Symbolication identifies `bitneedle_native_record_player_render_stereo_planar`. A Rust panic crossed its non-unwinding C interface and caused the abort.

The register state contains the normal 128-frame callback size. The report does not show an unusually large callback.

The report does not contain the first Rust panic message. It does not identify the first failed invariant.

### Investigated hypothesis

The production deck path contained `expect` calls for dynamic mechanical operations. A rejected operation could panic inside the audio callback.

The correction removes these dynamic panic sites. It also retains an exact typed diagnostic after each rejected operation.

A test starts normal playback at 2,000 record turns. It advances 48,000 samples without a rejected operation.

This result lowers confidence that long playback alone caused a deck rejection. The crash report still does not identify the first panic site.

### Required correction

Dynamic mechanical rejection must not unwind through the host callback. The renderer must keep the last valid transactional state for one sample.

The renderer must retry the current control on the next sample. It must count each recovery for later diagnosis.

Tests must force a rejected step. Tests must confirm finite and continuous fallback motion without a panic.

Tests must start from a large valid turn count. This condition represents long program playback without a long test file.

The native boundary must catch any Rust panic before it crosses the C interface. It must retain the first panic message and source location.

The application must report that diagnostic from a non-audio thread. It must then stop with the diagnostic in the next crash report.

### Remaining checks

- Run all `record-player` tests.
- Run the native C interface tests with the fixed crate.
- Run the iOS player tests with the release build configuration.
- Publish a new TestFlight build.
- Confirm that the same playback action does not create another crash report.

### Rejected conclusions

The report does not prove an out-of-memory failure. The report also does not prove a SwiftUI or application lifecycle failure.

The report does not prove that deck mechanics caused the first panic. Keep the callback recovery even if later evidence identifies another invariant.

## Incident IOS-2026-08-02-1652

### Report identity

- The TestFlight feedback comment is `1652`.
- The feedback identifier is `AGMZ1FuxDGkhZ9X5ms66lkQ`.
- The incident identifier is `71316197-F176-4A4C-ABE2-11C0821A273C`.
- The application version is `1.0 (22)`.
- The device is an iPhone 13 mini.
- The operating system is iOS 26.5.2.
- The crash occurred at 2026-08-02 16:52:32 Europe/London.

### User action

Playback was active. The user changed the record format from 45 to LP.

The application must support this action. A user can change design controls while listening to the record.

### Confirmed evidence

The process received `SIGABRT` on audio render thread 12. The callback requested stereo planar program audio.

The archived dSYM matches the application image. Symbolication identifies the same native render function as the 15:56 incident.

The Rust stack contains `panic_in_cleanup`. It also contains allocation and symbol-map frames from panic reporting.

The Rust default panic reporter ran on the real-time audio thread. That reporter tried to allocate and symbolize a backtrace.

The reporting path failed during panic cleanup. The process aborted before the original panic message reached the application.

### Correction

The native boundary now marks each guarded render call with thread-local state. The panic hook writes the first diagnostic into fixed memory.

The hook does not call the default reporter during a guarded render. Non-render Rust panics keep the normal reporter.

The boundary catches the render panic and emits silence for that quantum. A non-audio timer reads the retained diagnostic.

The application saves the diagnostic before it stops. The final failure message contains the Rust message and source location.

The deck correction remains necessary. A format change can alter motor controls while the powered brake tail is active.

### Remaining evidence gap

The report still does not contain the original Rust panic message. It cannot confirm the first failed invariant.

The next TestFlight build must identify that invariant if the format change still fails.

## Incident IOS-2026-08-02-1701

### Report identity

- The TestFlight feedback comment is `1701`.
- The feedback identifier is `AN8oSAR4eW1mWJ8Ansj_FK8`.
- The incident identifier is `28DBC8D4-F48F-4D10-93C9-55230F1AB1C9`.
- The application version is `1.0 (22)`.
- The device is an iPhone 13 mini.
- The operating system is iOS 26.5.2.
- The crash occurred at 2026-08-02 17:01:19 Europe/London.
- The application launched at 16:53:59.

### Confirmed evidence

The process received `SIGABRT` on audio render thread 6. The callback requested stereo planar program audio.

The archived dSYM matches the application image. Both UUIDs are `315AB821-85A7-370C-AFC1-E10D49A42256`.

Symbolication identifies `bitneedle_native_record_player_render_stereo_planar`. The Swift call site is `NativeRecordAudioPipeline.swift:842`.

The Rust stack contains `panic_in_cleanup`. It also contains panic reporter allocation and symbol-map frames.

This signature matches the 16:52 incident. The report does not contain the original Rust panic message.

### Release status

Build 22 used `record-player` commit `129fdea1`. That commit did not contain the dynamic deck recovery correction.

TestFlight build 23 uses `record-player` commit `4dd487bf`. It includes deck recovery and guarded native rendering.

Build 23 also saves the exact first Rust panic diagnostic. App Store Connect marked build 23 as `VALID`.

### Required verification

Repeat normal playback and format changes with build 23. Confirm that no equivalent crash occurs.

If build 23 stops, use its saved diagnostic as the primary cause evidence.
