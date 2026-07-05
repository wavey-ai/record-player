# player-wasm

Player-only WebAssembly facade for Bitneedle PNG extraction and ECDC decoding.

The decoder worker uses these exports:

- `initPanicHook`
- `playerWasmBuildInfoJson`
- `decodeRecordMetadataJson`
- `decodeRecordProgrammeMapJson`
- `inferRecordProfileFromPng`
- `decodeRecordPngToPayload` and its profile/length variants
- `ecdcMetadata`
- `ecdcChunkLayoutFromMetadata`
- `ecdcCropDecodedOwnedAudio`
- `lmEcdcDecodeChunks`
- `QuantizedLmChunkDecoder`
- `normalizeRecordProfileName`
- `recordPlaybackMetadataFromHeaderJson`
- `resolvePlaybackPayloadMetadataJson`
- `createPlayerEcdcCacheProofContextJson`

Record authoring, rendering, sidecar tooling, remote scratch identity, wallet helpers and application-only utilities are intentionally not exported.
