"use strict";

(() => {
  function requirePlayerPcmHelperFunction(value, name) {
    if (typeof value !== "function") {
      throw new Error(`Bitneedle player PCM helpers require ${name}.`);
    }
    return value;
  }

  function createPlayerPcmHelpers({
    opusCacheBitrate = 64000,
    opusCacheFrameRate = 50,
    clamp,
    getScratchAudioContext,
    ensureLibopusModule,
    // Optional: when provided, Opus cache encode uses soundkit's WasmOpusEncoder
    // (pure-Rust libopus-rs) instead of the standalone libopus bundle. Press
    // injects this; the player currently omits it and falls back to libopus.
    ensureSoundkitOpusEncoderModule = null,
    buildSoundkitFrameHeader,
    uint8View,
    yieldToMainThread,
    assertPlayerRuntimeActive,
    concatenateUint8Chunks,
    soundkitOpusPacketItemsFromPackets,
    decodeBase64ToUint8Array,
  } = {}) {
    clamp = requirePlayerPcmHelperFunction(clamp, "clamp");
    getScratchAudioContext = requirePlayerPcmHelperFunction(getScratchAudioContext, "getScratchAudioContext");
    ensureLibopusModule = requirePlayerPcmHelperFunction(ensureLibopusModule, "ensureLibopusModule");
    buildSoundkitFrameHeader = requirePlayerPcmHelperFunction(buildSoundkitFrameHeader, "buildSoundkitFrameHeader");
    uint8View = requirePlayerPcmHelperFunction(uint8View, "uint8View");
    yieldToMainThread = requirePlayerPcmHelperFunction(yieldToMainThread, "yieldToMainThread");
    assertPlayerRuntimeActive = requirePlayerPcmHelperFunction(assertPlayerRuntimeActive, "assertPlayerRuntimeActive");
    concatenateUint8Chunks = requirePlayerPcmHelperFunction(concatenateUint8Chunks, "concatenateUint8Chunks");
    soundkitOpusPacketItemsFromPackets = requirePlayerPcmHelperFunction(
      soundkitOpusPacketItemsFromPackets,
      "soundkitOpusPacketItemsFromPackets",
    );
    decodeBase64ToUint8Array = requirePlayerPcmHelperFunction(decodeBase64ToUint8Array, "decodeBase64ToUint8Array");

    function isS16PcmWindowProvider(source) {
      return source?.format === "s16-pcm-window-provider";
    }

    function floatToS16Sample(value) {
      const sample = Math.max(-1, Math.min(1, Number(value) || 0));
      return sample < 0 ? Math.round(sample * 32768) : Math.round(sample * 32767);
    }

    function s16ToFloatSample(value) {
      return Math.max(-1, Math.min(1, (Number(value) || 0) / 32768));
    }

    function createS16PcmWindowProvider({ channelData, sampleRate = 48000, length = 0, numberOfChannels = 0, peaks = null, audioFormat = "", decodedRanges = null } = {}) {
      const channels = (Array.isArray(channelData) ? channelData : [])
        .map((channel) => channel instanceof Int16Array ? channel : new Int16Array(channel || 0))
        .filter((channel) => channel.length > 0);
      const safeChannels = Math.max(1, Math.floor(Number(numberOfChannels) || channels.length || 1));
      const safeLength = Math.max(
        0,
        Math.floor(Number(length) || channels.reduce((max, channel) => Math.max(max, channel.length), 0)),
      );
      while (channels.length < safeChannels) {
        channels.push(channels[channels.length - 1] || new Int16Array(safeLength));
      }
      const safeSampleRate = Math.max(1, Math.floor(Number(sampleRate) || 48000));
      // Progressive record decode uses the same PCM provider shape as complete
      // decode, but starts with empty decodedRanges. Consumers can render the
      // full timeline immediately while playback/scratch reads are bounded to
      // the ranges that have actually arrived.
      const ranges = Array.isArray(decodedRanges)
        ? []
        : safeLength > 0
          ? [{ start: 0, end: safeLength }]
          : [];

      function mergeDecodedRange(startFrame, endFrame) {
        const start = clamp(Math.floor(Number(startFrame) || 0), 0, safeLength);
        const end = clamp(Math.floor(Number(endFrame) || 0), 0, safeLength);
        if (!(end > start)) {
          return ranges;
        }
        ranges.push({ start, end });
        ranges.sort((left, right) => left.start - right.start);
        for (let index = 0; index < ranges.length - 1;) {
          const current = ranges[index];
          const next = ranges[index + 1];
          if (next.start <= current.end + 1) {
            current.end = Math.max(current.end, next.end);
            ranges.splice(index + 1, 1);
          } else {
            index += 1;
          }
        }
        return ranges;
      }

      function hasDecodedFrame(frame) {
        const index = Math.floor(Number(frame) || 0);
        return ranges.some((range) => index >= range.start && index < range.end);
      }

      function hasDecodedAudio() {
        return ranges.some((range) => range.end > range.start);
      }

      function decodedFrameEndAt(frame) {
        const index = Math.floor(Number(frame) || 0);
        const range = ranges.find((candidate) => index >= candidate.start && index < candidate.end);
        return range ? range.end : index;
      }

      function clampToDecodedFrame(frame, { preferEnd = false } = {}) {
        const index = clamp(Math.floor(Number(frame) || 0), 0, Math.max(0, safeLength - 1));
        if (!ranges.length) {
          return index;
        }
        for (const range of ranges) {
          if (index >= range.start && index < range.end) {
            return index;
          }
        }
        if (preferEnd && index >= ranges[ranges.length - 1].end) {
          return Math.max(0, ranges[ranges.length - 1].end - 1);
        }
        const nextRange = ranges.find((range) => index < range.start);
        if (nextRange) {
          return nextRange.start;
        }
        return Math.max(0, ranges[ranges.length - 1].end - 1);
      }

      function copyDecodedS16ToFloat(target, source, start, windowLength) {
        target.fill(0);
        const end = start + windowLength;
        for (const range of ranges) {
          const copyStart = Math.max(start, range.start);
          const copyEnd = Math.min(end, range.end);
          if (!(copyEnd > copyStart)) {
            continue;
          }
          const targetOffset = copyStart - start;
          for (let index = copyStart; index < copyEnd; index += 1) {
            target[targetOffset + index - copyStart] = s16ToFloatSample(source[index] || 0);
          }
        }
      }

      const provider = {
        format: "s16-pcm-window-provider",
        sampleRate: safeSampleRate,
        numberOfChannels: safeChannels,
        length: safeLength,
        duration: safeLength > 0 ? safeLength / safeSampleRate : 0,
        channelData: channels,
        decodedRanges: ranges,
        peaks: peaks || null,
        audioFormat,
        markDecodedRange: mergeDecodedRange,
        hasDecodedAudio,
        hasDecodedFrame,
        decodedFrameEndAt,
        clampToDecodedFrame,
        async createChannelWindow(startFrame = 0, frameCount = safeLength) {
          const start = clamp(Math.floor(Number(startFrame) || 0), 0, Math.max(0, safeLength));
          const requested = Math.max(1, Math.floor(Number(frameCount) || 1));
          const end = Math.min(safeLength, start + requested);
          const windowLength = Math.max(1, end - start);
          const output = Array.from({ length: safeChannels }, (_, channelIndex) => {
            const source = channels[Math.min(channelIndex, channels.length - 1)] || channels[0];
            const target = new Float32Array(windowLength);
            copyDecodedS16ToFloat(target, source, start, windowLength);
            return target;
          });
          return {
            channelData: output,
            sampleRate: safeSampleRate,
            start,
            end,
            totalFrames: safeLength,
          };
        },
        async copyChannelWindowTo(targetChannels, startFrame = 0, frameCount = safeLength) {
          const start = clamp(Math.floor(Number(startFrame) || 0), 0, Math.max(0, safeLength));
          const requested = Math.max(1, Math.floor(Number(frameCount) || 1));
          const end = Math.min(safeLength, start + requested);
          const windowLength = Math.max(0, end - start);
          for (let channelIndex = 0; channelIndex < targetChannels.length; channelIndex += 1) {
            const target = targetChannels[channelIndex];
            if (!target) {
              continue;
            }
            target.fill(0);
            const source = channels[Math.min(channelIndex, channels.length - 1)] || channels[0];
            const copyLength = Math.min(target.length, windowLength);
            copyDecodedS16ToFloat(target, source, start, copyLength);
          }
          return {
            sampleRate: safeSampleRate,
            start,
            end,
            totalFrames: safeLength,
          };
        },
        async createAudioBufferWindow(startFrame = 0, frameCount = safeLength) {
          const context = getScratchAudioContext();
          if (!context) {
            throw new Error("Audio context unavailable for PCM window playback.");
          }
          const windowPayload = await provider.createChannelWindow(startFrame, frameCount);
          const output = context.createBuffer(
            safeChannels,
            Math.max(1, windowPayload.channelData[0]?.length || 1),
            safeSampleRate,
          );
          for (let channelIndex = 0; channelIndex < safeChannels; channelIndex += 1) {
            output.getChannelData(channelIndex).set(windowPayload.channelData[channelIndex]);
          }
          return output;
        },
      };
      return provider;
    }

    function createS16PcmWindowProviderFromPcmBytes({ pcmBytes, frameCount, sampleRate, channels, bitsPerSample, peaks = null, audioFormat = "" }) {
      const source = pcmBytes instanceof Uint8Array ? pcmBytes : new Uint8Array(pcmBytes || 0);
      const safeChannels = Math.max(1, Math.floor(Number(channels) || 1));
      const safeFrameCount = Math.max(0, Math.floor(Number(frameCount) || 0));
      const safeBits = Math.floor(Number(bitsPerSample) || 16);
      if (safeBits !== 16) {
        throw new Error(`Unsupported decoded Opus PCM depth: ${bitsPerSample}`);
      }
      const view = new DataView(source.buffer, source.byteOffset, source.byteLength);
      const channelData = Array.from({ length: safeChannels }, () => new Int16Array(safeFrameCount));
      let byteOffset = 0;
      for (let frame = 0; frame < safeFrameCount; frame += 1) {
        for (let channel = 0; channel < safeChannels; channel += 1) {
          channelData[channel][frame] = byteOffset + 2 <= view.byteLength ? view.getInt16(byteOffset, true) : 0;
          byteOffset += 2;
        }
      }
      return createS16PcmWindowProvider({
        channelData,
        sampleRate,
        length: safeFrameCount,
        numberOfChannels: safeChannels,
        peaks,
        audioFormat,
      });
    }

    function createProgressiveS16PcmWindowProvider({ sampleRate = 48000, length = 0, numberOfChannels = 0, audioFormat = "" } = {}) {
      // This is the bridge between worker progress and the live player: each
      // decoded ECDC chunk is copied into its final absolute PCM position, then
      // marked as available. Undecoded holes remain silent and non-scratchable.
      const safeLength = Math.max(0, Math.floor(Number(length) || 0));
      const safeChannels = Math.max(1, Math.floor(Number(numberOfChannels) || 1));
      const provider = createS16PcmWindowProvider({
        channelData: Array.from({ length: safeChannels }, () => new Int16Array(safeLength)),
        sampleRate,
        length: safeLength,
        numberOfChannels: safeChannels,
        audioFormat,
        decodedRanges: [],
      });

      function addS16Segment(segment = {}) {
        const start = clamp(Math.floor(Number(segment.startFrame) || 0), 0, safeLength);
        const end = clamp(Math.floor(Number(segment.endFrame) || start), start, safeLength);
        if (!(end > start)) {
          return provider;
        }
        const sourceBuffers = Array.isArray(segment.channelBuffers) ? segment.channelBuffers : [];
        for (let channelIndex = 0; channelIndex < safeChannels; channelIndex += 1) {
          const target = provider.channelData[channelIndex];
          const sourceBuffer = sourceBuffers[Math.min(channelIndex, sourceBuffers.length - 1)] || sourceBuffers[0];
          const source = sourceBuffer instanceof Int16Array ? sourceBuffer : new Int16Array(sourceBuffer || 0);
          target.set(source.subarray(0, Math.min(source.length, end - start)), start);
        }
        provider.markDecodedRange(start, end);
        return provider;
      }

      provider.addS16Segment = addS16Segment;
      provider.addS16Segments = (segments = []) => {
        if (Array.isArray(segments)) {
          segments.forEach(addS16Segment);
        }
        return provider;
      };
      return provider;
    }

    function playerOpusCacheBitrate(_meta) {
      return opusCacheBitrate;
    }

    function playerOpusCacheFrameSize(sampleRate) {
      return Math.max(1, Math.round((Number(sampleRate) || 48000) / opusCacheFrameRate));
    }

    function pcmFloatToInt16Sample(value) {
      const sample = Math.max(-1, Math.min(1, Number(value) || 0));
      return sample < 0 ? Math.round(sample * 32768) : Math.round(sample * 32767);
    }

    async function encodePlanarFloatSegmentToSoundkitOpusPackets(segment, meta, startFrame = 0) {
      const sampleRate = Number(meta.sample_rate) || 48000;
      const channels = Number(meta.channels) || 2;
      const segmentSamples = Number(meta.segment_samples) || Math.floor(segment.length / channels);
      const frameSize = playerOpusCacheFrameSize(sampleRate);
      const bitrate = playerOpusCacheBitrate(meta);
      const opus = await ensureLibopusModule();
      const encoder = new opus.Encoder(channels, sampleRate, bitrate, frameSize);
      const source = segment instanceof Float32Array ? segment : new Float32Array(segment);
      const frame = new Int16Array(frameSize * channels);
      const packets = [];
      const frameCount = Math.ceil(segmentSamples / frameSize);

      try {
        for (let packetIndex = 0; packetIndex < frameCount; packetIndex += 1) {
          const sourceFrame = packetIndex * frameSize;
          const copyFrames = Math.min(frameSize, segmentSamples - sourceFrame);
          frame.fill(0);
          for (let sampleIndex = 0; sampleIndex < copyFrames; sampleIndex += 1) {
            for (let channel = 0; channel < channels; channel += 1) {
              frame[(sampleIndex * channels) + channel] = pcmFloatToInt16Sample(
                source[(channel * segmentSamples) + sourceFrame + sampleIndex],
              );
            }
          }
          const result = encoder.enc_frame(frame);
          if (!result?.ok || !result.encodedData?.byteLength) {
            throw new Error(`Wavey Rust Opus encode failed at packet ${packetIndex + 1}/${frameCount}.`);
          }
          const pts = Math.max(0, Math.round(Number(startFrame) || 0)) + sourceFrame;
          const header = buildSoundkitFrameHeader({
            encoding: 2,
            sampleRate,
            channels,
            bitsPerSample: 16,
            sampleSize: frameSize,
            payloadSize: result.encodedData.byteLength,
            pts,
            packetHeaderVersion: 2,
          });
          const packet = new Uint8Array(header.length + result.encodedData.byteLength);
          packet.set(header, 0);
          packet.set(result.encodedData, header.length);
          packets.push(packet.buffer);
        }
      } finally {
        encoder.destroy?.();
        if (encoder.in) {
          opus._opus_free?.(encoder.in);
        }
        if (encoder.out) {
          opus._opus_free?.(encoder.out);
        }
      }

      return packets;
    }

    async function encodeS16PcmWindowProviderToSoundkitOpusPackets(provider, meta = {}, startFrame = 0) {
      if (!isS16PcmWindowProvider(provider)) {
        throw new Error("Decoded ECDC cache requires an S16 PCM provider.");
      }
      const sampleRate = Math.max(1, Number(provider.sampleRate) || Number(meta.sample_rate) || 48000);
      const channels = Math.max(1, Number(provider.numberOfChannels) || Number(meta.channels) || 2);
      const frameCount = Math.max(0, Math.floor(Number(provider.length) || 0));
      if (sampleRate !== 48000) {
        throw new Error(`Wavey Rust Opus cache encode requires 48 kHz PCM, got ${sampleRate}.`);
      }
      if (!(frameCount > 0)) {
        return [];
      }
      const frameSize = playerOpusCacheFrameSize(sampleRate);
      const bitrate = playerOpusCacheBitrate(meta);
      // Prefer soundkit's WasmOpusEncoder (pure-Rust libopus-rs, the single
      // encoder shared with decode) when a soundkit module is injected;
      // otherwise fall back to the standalone libopus-rs bundle. Both emit
      // identical V1 soundkit Opus packets (same libopus-rs underneath), so the
      // ECDC-keyed cache stays byte-compatible and player-readable either way.
      const soundkitModule = ensureSoundkitOpusEncoderModule
        ? await ensureSoundkitOpusEncoderModule()
        : null;
      const opus = soundkitModule ? null : await ensureLibopusModule();
      const soundkitEncoder = soundkitModule
        ? new soundkitModule.WasmOpusEncoder(sampleRate, channels, bitrate, frameSize)
        : null;
      const encoder = soundkitEncoder || new opus.Encoder(channels, sampleRate, bitrate, frameSize);
      if (!soundkitEncoder) encoder.set_vbr?.(false);
      const frame = new Int16Array(frameSize * channels);
      const packets = [];
      const packetCount = Math.ceil(frameCount / frameSize);

      try {
        for (let packetIndex = 0; packetIndex < packetCount; packetIndex += 1) {
          const sourceFrame = packetIndex * frameSize;
          const copyFrames = Math.min(frameSize, frameCount - sourceFrame);
          frame.fill(0);
          for (let sampleIndex = 0; sampleIndex < copyFrames; sampleIndex += 1) {
            for (let channel = 0; channel < channels; channel += 1) {
              const source = provider.channelData?.[Math.min(channel, provider.channelData.length - 1)];
              frame[(sampleIndex * channels) + channel] = source?.[sourceFrame + sampleIndex] || 0;
            }
          }
          let encoded;
          if (soundkitEncoder) {
            encoded = uint8View(soundkitEncoder.encodeInterleavedI16(frame));
          } else {
            const result = encoder.enc_frame(frame);
            if (!result?.ok || !result.encodedData?.byteLength) {
              throw new Error(`Wavey Rust Opus CBR cache encode failed at packet ${packetIndex + 1}/${packetCount}.`);
            }
            encoded = uint8View(result.encodedData);
          }
          if (!encoded?.byteLength) {
            throw new Error(`Wavey Rust Opus CBR cache encode failed at packet ${packetIndex + 1}/${packetCount}.`);
          }
          const pts = Math.max(0, Math.round(Number(startFrame) || 0)) + sourceFrame;
          const header = buildSoundkitFrameHeader({
            encoding: 2,
            sampleRate,
            channels,
            bitsPerSample: 16,
            sampleSize: frameSize,
            payloadSize: encoded.byteLength,
            pts,
            packetHeaderVersion: 2,
          });
          const packet = new Uint8Array(header.length + encoded.byteLength);
          packet.set(header, 0);
          packet.set(encoded, header.length);
          packets.push(packet.buffer);
          if (packetIndex % 16 === 15) {
            await yieldToMainThread();
            assertPlayerRuntimeActive();
          }
        }
      } finally {
        if (soundkitEncoder) {
          soundkitEncoder.free?.();
        } else {
          encoder.destroy?.();
          if (encoder.in) {
            opus._opus_free?.(encoder.in);
          }
          if (encoder.out) {
            opus._opus_free?.(encoder.out);
          }
        }
      }

      return packets;
    }

    async function decodeSoundkitOpusStreamToPcmBytesWithWebStudioOpus(stream, packetItems, {
      sampleRate,
      channels,
      expectedFrameCount,
    }) {
      const opus = await ensureLibopusModule();
      const maxPacketFrames = packetItems.reduce(
        (maxFrames, item) => Math.max(maxFrames, Number(item?.header?.sampleSize) || 0),
        0,
      );
      const frameSize = Math.max(
        1,
        maxPacketFrames,
        playerOpusCacheFrameSize(sampleRate),
      );
      const decoder = new opus.Decoder(channels, sampleRate, frameSize);
      try {
        const outputChunks = [];
        let outputFrameCount = 0;
        for (const item of packetItems) {
          const decoded = decoder.dec_frame(item.payload);
          const decodedFrameCount = Math.max(0, Number(decoded.decodedSize) || 0);
          const remainingFrames = Math.max(0, (expectedFrameCount || decodedFrameCount) - outputFrameCount);
          const framesToCopy = expectedFrameCount ? Math.min(decodedFrameCount, remainingFrames) : decodedFrameCount;
          if (framesToCopy > 0) {
            const pcmChunk = new Uint8Array(framesToCopy * channels * 2);
            const pcmView = new DataView(pcmChunk.buffer);
            const source = decoded.output || [];
            for (let index = 0; index < framesToCopy * channels; index += 1) {
              const sample = Math.max(-32768, Math.min(32767, Number(source[index]) || 0));
              pcmView.setInt16(index * 2, sample, true);
            }
            outputChunks.push(pcmChunk);
            outputFrameCount += framesToCopy;
          }
          if (expectedFrameCount && outputFrameCount >= expectedFrameCount) {
            break;
          }
        }
        return concatenateUint8Chunks(outputChunks, outputFrameCount * channels * 2);
      } finally {
        decoder.destroy?.();
        if (decoder.in) {
          opus._opus_free?.(decoder.in);
        }
        if (decoder.out) {
          opus._opus_free?.(decoder.out);
        }
      }
    }

    async function decodeSoundkitOpusPacketsToPcmBytes(stream, packets) {
      const packetItems = soundkitOpusPacketItemsFromPackets(packets);

      if (!packetItems.length) {
        return new Uint8Array(0);
      }

      const firstHeader = packetItems[0].header;
      const sampleRate = Number(stream.sampleRate) || firstHeader.sampleRate;
      const channels = Number(stream.channels) || firstHeader.channels;
      const startFrame = Math.max(0, Number(stream.startFrame) || 0);
      const packetFrameCount = packetItems.reduce((sum, item) => sum + item.header.sampleSize, 0);
      const declaredFrameCount = Math.max(0, (Number(stream.endFrame) || startFrame) - startFrame);
      const expectedFrameCount = declaredFrameCount || packetFrameCount;

      return decodeSoundkitOpusStreamToPcmBytesWithWebStudioOpus(stream, packetItems, {
        sampleRate,
        channels,
        expectedFrameCount,
      });
    }

    async function decodeSoundkitOpusStreamToPcmBytes(stream) {
      const packets = (stream.packetsBase64 || []).map(decodeBase64ToUint8Array);
      return decodeSoundkitOpusPacketsToPcmBytes(stream, packets);
    }

    return {
      createProgressiveS16PcmWindowProvider,
      createS16PcmWindowProvider,
      createS16PcmWindowProviderFromPcmBytes,
      decodeSoundkitOpusPacketsToPcmBytes,
      decodeSoundkitOpusStreamToPcmBytes,
      encodePlanarFloatSegmentToSoundkitOpusPackets,
      encodeS16PcmWindowProviderToSoundkitOpusPackets,
      floatToS16Sample,
      isS16PcmWindowProvider,
      playerOpusCacheBitrate,
      playerOpusCacheFrameSize,
      s16ToFloatSample,
    };
  }

  globalThis.BitneedlePlayerPcmHelpers = {
    createPlayerPcmHelpers,
  };
})();
