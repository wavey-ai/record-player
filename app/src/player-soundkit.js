function uint8View(value) {
  if (value instanceof Uint8Array) {
    return value;
  }
  if (ArrayBuffer.isView(value)) {
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  }
  return new Uint8Array(value || 0);
}

export function decodeSoundkitFrameHeader(data) {
  if (!(data instanceof Uint8Array) || data.length < 4) {
    throw new Error("Invalid SoundKit frame header.");
  }
  const header = new DataView(data.buffer, data.byteOffset, 4).getUint32(0, false);
  const magicWord = header >>> 26;
  if (magicWord === 0x2b) {
    return decodeSoundkitFrameHeaderV2(data, header);
  }
  if (magicWord !== 0x2a) {
    throw new Error("Invalid SoundKit frame magic.");
  }
  const sampleRate = [16000, 44100, 48000, 96000][(header >>> 24) & 0x3];
  const bitsPerSample = [16, 24, 32][(header >>> 22) & 0x3];
  const hasPts = ((header >>> 21) & 0x1) === 1;
  const hasId = ((header >>> 20) & 0x1) === 1;
  const encoding = (header >>> 17) & 0x7;
  const endianness = (header >>> 16) & 0x1;
  const channels = ((header >>> 12) & 0xf) + 1;
  const sampleSize = header & 0xfff;
  let offset = 4;
  let id = null;
  let pts = null;
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  if (!sampleRate || !bitsPerSample) {
    throw new Error("Unsupported SoundKit frame format.");
  }
  if (hasId) {
    if (data.length < offset + 8) throw new Error("Truncated SoundKit frame id.");
    id = view.getBigUint64(offset, false);
    offset += 8;
  }
  if (hasPts) {
    if (data.length < offset + 8) throw new Error("Truncated SoundKit frame pts.");
    pts = view.getBigUint64(offset, false);
    offset += 8;
  }
  return {
    encoding,
    endianness,
    sampleRate,
    channels,
    bitsPerSample,
    sampleSize,
    payloadSize: Math.max(0, data.byteLength - offset),
    id,
    pts,
    headerSize: offset,
    packetHeaderVersion: 1,
  };
}

function decodeSoundkitFrameHeaderV2(data, header) {
  if (data.length < 8) {
    throw new Error("Truncated SoundKit v2 frame header.");
  }
  const version = (header >>> 24) & 0x3;
  if (version !== 2) {
    throw new Error("Invalid SoundKit v2 frame version.");
  }
  const flags = (header >>> 16) & 0xff;
  const encoding = (header >>> 12) & 0xf;
  const sampleRate = [
    8000,
    12000,
    16000,
    24000,
    32000,
    44100,
    48000,
    88200,
    96000,
    176400,
    192000,
  ][(header >>> 8) & 0xf];
  const channels = ((header >>> 3) & 0x1f) + 1;
  const bitsPerSample = [0, 8, 16, 24, 32, 64][header & 0x7];
  if (!sampleRate || bitsPerSample == null) {
    throw new Error("Unsupported SoundKit v2 frame format.");
  }
  const hasId = (flags & 0x01) !== 0;
  const idIsU64 = (flags & 0x02) !== 0;
  const hasPts = (flags & 0x04) !== 0;
  const hasCrc = (flags & 0x08) !== 0;
  const bigEndian = (flags & 0x10) !== 0;
  const extendedSizes = (flags & 0x20) !== 0;
  if (idIsU64 && !hasId) {
    throw new Error("Invalid SoundKit v2 frame id flags.");
  }
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const sizeWord = view.getUint32(4, false);
  let payloadSize;
  let sampleSize;
  let offset = 8;
  if (extendedSizes) {
    if (sizeWord !== 0xffffffff || data.length < 16) {
      throw new Error("Invalid SoundKit v2 extended size header.");
    }
    payloadSize = view.getUint32(8, false);
    sampleSize = view.getUint32(12, false);
    offset = 16;
  } else {
    payloadSize = (sizeWord >>> 16) & 0xffff;
    sampleSize = sizeWord & 0xffff;
    if (payloadSize === 0xffff || sampleSize === 0xffff) {
      throw new Error("Invalid SoundKit v2 short size sentinel.");
    }
  }
  let id = null;
  let pts = null;
  if (hasId) {
    if (idIsU64) {
      if (data.length < offset + 8) throw new Error("Truncated SoundKit v2 frame id.");
      id = view.getBigUint64(offset, false);
      offset += 8;
    } else {
      if (data.length < offset + 4) throw new Error("Truncated SoundKit v2 frame id.");
      id = BigInt(view.getUint32(offset, false));
      offset += 4;
    }
  }
  if (hasPts) {
    if (data.length < offset + 8) throw new Error("Truncated SoundKit v2 frame pts.");
    pts = view.getBigUint64(offset, false);
    offset += 8;
  }
  let packetCrc32 = null;
  if (hasCrc) {
    if (data.length < offset + 4) throw new Error("Truncated SoundKit v2 frame crc.");
    packetCrc32 = view.getUint32(offset, false);
    offset += 4;
  }
  if (payloadSize <= 0 || sampleSize <= 0 || data.length < offset + payloadSize) {
    throw new Error("Invalid SoundKit v2 Opus payload.");
  }
  return {
    encoding,
    endianness: bigEndian ? 1 : 0,
    sampleRate,
    channels,
    bitsPerSample,
    sampleSize,
    payloadSize,
    id,
    pts,
    packetCrc32,
    headerSize: offset,
    packetHeaderVersion: 2,
  };
}

function soundkitSampleRateIndex(sampleRate) {
  const rates = [16000, 44100, 48000, 96000];
  const index = rates.indexOf(Number(sampleRate) || 48000);
  return index >= 0 ? index : 2;
}

function soundkitSampleRateIndexV2(sampleRate) {
  const rates = [
    8000,
    12000,
    16000,
    24000,
    32000,
    44100,
    48000,
    88200,
    96000,
    176400,
    192000,
  ];
  const index = rates.indexOf(Number(sampleRate) || 48000);
  return index >= 0 ? index : 6;
}

function soundkitBitsPerSampleIndexV2(bitsPerSample) {
  const bits = [0, 8, 16, 24, 32, 64];
  const index = bits.indexOf(Number(bitsPerSample) || 16);
  return index >= 0 ? index : 2;
}

export function buildSoundkitFrameHeader({
  encoding = 2,
  sampleRate = 48000,
  channels = 2,
  bitsPerSample = 16,
  sampleSize = 0,
  payloadSize = 0,
  pts = null,
  packetHeaderVersion = 2,
} = {}) {
  if (packetHeaderVersion === 2 || payloadSize > 0xffff || sampleSize > 0xfff) {
    const normalizedPayloadSize = Math.max(0, Math.floor(Number(payloadSize) || 0));
    const normalizedSampleSize = Math.max(0, Math.floor(Number(sampleSize) || 0));
    if (!(normalizedPayloadSize > 0) || !(normalizedSampleSize > 0)) {
      throw new Error("SoundKit v2 frame header requires payloadSize and sampleSize.");
    }
    const useExtendedSizes = normalizedPayloadSize > 0xfffe || normalizedSampleSize > 0xfffe;
    const hasPts = pts != null;
    const flags = (hasPts ? 0x04 : 0) | (useExtendedSizes ? 0x20 : 0);
    const headerSize = (useExtendedSizes ? 16 : 8) + (hasPts ? 8 : 0);
    const output = new Uint8Array(headerSize);
    const view = new DataView(output.buffer);
    const header =
      (0x2b << 26) |
      (2 << 24) |
      ((flags & 0xff) << 16) |
      ((encoding & 0x0f) << 12) |
      ((soundkitSampleRateIndexV2(sampleRate) & 0x0f) << 8) |
      ((Math.max(1, Math.min(32, Number(channels) || 1)) - 1) << 3) |
      (soundkitBitsPerSampleIndexV2(bitsPerSample) & 0x07);
    view.setUint32(0, header >>> 0, false);
    let offset = 8;
    if (useExtendedSizes) {
      view.setUint32(4, 0xffffffff, false);
      view.setUint32(8, normalizedPayloadSize, false);
      view.setUint32(12, normalizedSampleSize, false);
      offset = 16;
    } else {
      view.setUint32(
        4,
        (((normalizedPayloadSize & 0xffff) << 16) | (normalizedSampleSize & 0xffff)) >>> 0,
        false,
      );
    }
    if (hasPts) {
      view.setBigUint64(offset, BigInt(Math.max(0, Math.round(Number(pts) || 0))), false);
    }
    return output;
  }

  const bitsIndex = bitsPerSample === 24 ? 1 : bitsPerSample === 32 ? 2 : 0;
  const hasPts = pts != null;
  const headerSize = 4 + (hasPts ? 8 : 0);
  const output = new Uint8Array(headerSize);
  const header =
    (0x2a << 26) |
    (soundkitSampleRateIndex(sampleRate) << 24) |
    (bitsIndex << 22) |
    ((hasPts ? 1 : 0) << 21) |
    ((encoding & 0x7) << 17) |
    ((Math.max(1, Math.min(16, Number(channels) || 1)) - 1) << 12) |
    (Math.max(0, Math.min(4095, Math.round(Number(sampleSize) || 0))) & 0xfff);
  const view = new DataView(output.buffer);
  view.setUint32(0, header >>> 0, false);
  if (hasPts) {
    view.setBigUint64(4, BigInt(Math.max(0, Math.round(Number(pts) || 0))), false);
  }
  return output;
}

export function soundkitOpusPacketItemsFromPackets(packets) {
  const items = [];
  for (const packetBytes of Array.isArray(packets) ? packets : []) {
    const source = uint8View(packetBytes);
    let offset = 0;
    while (offset < source.byteLength) {
      const remaining = source.subarray(offset);
      const header = decodeSoundkitFrameHeader(remaining);
      if (header.encoding !== 2) {
        throw new Error("SoundKit packet is not Opus.");
      }
      const packetSize = header.headerSize + header.payloadSize;
      if (packetSize <= 0 || packetSize > remaining.byteLength) {
        throw new Error(
          `Truncated SoundKit Opus packet at byte ${offset}: need ${packetSize}, have ${remaining.byteLength}.`,
        );
      }
      items.push({
        header,
        payload: source.slice(offset + header.headerSize, offset + packetSize),
      });
      offset += packetSize;
    }
  }
  return items;
}
