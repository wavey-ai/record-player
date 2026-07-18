// AudioWorkletGlobalScope lacks TextDecoder/TextEncoder, which the
// wasm-bindgen glue instantiates at module top level. Minimal UTF-8
// implementations, installed before the glue module evaluates (this module
// is imported first; ES module evaluation order guarantees it runs first).
if (typeof globalThis.TextDecoder === "undefined") {
  globalThis.TextDecoder = class TextDecoder {
    constructor() {}
    decode(input) {
      const bytes = input instanceof Uint8Array ? input : new Uint8Array(input || 0);
      let out = "";
      let i = 0;
      while (i < bytes.length) {
        const b0 = bytes[i++];
        if (b0 < 0x80) {
          out += String.fromCharCode(b0);
        } else if (b0 < 0xe0) {
          out += String.fromCharCode(((b0 & 0x1f) << 6) | (bytes[i++] & 0x3f));
        } else if (b0 < 0xf0) {
          out += String.fromCharCode(((b0 & 0x0f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f));
        } else {
          const cp = (((b0 & 0x07) << 18) | ((bytes[i++] & 0x3f) << 12) | ((bytes[i++] & 0x3f) << 6) | (bytes[i++] & 0x3f)) - 0x10000;
          out += String.fromCharCode(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff));
        }
      }
      return out;
    }
  };
}

if (typeof globalThis.TextEncoder === "undefined") {
  globalThis.TextEncoder = class TextEncoder {
    encode(text = "") {
      const out = [];
      for (let i = 0; i < text.length; i += 1) {
        let cp = text.charCodeAt(i);
        if (cp >= 0xd800 && cp < 0xdc00 && i + 1 < text.length) {
          const lo = text.charCodeAt(i + 1);
          if (lo >= 0xdc00 && lo < 0xe000) {
            cp = 0x10000 + ((cp - 0xd800) << 10) + (lo - 0xdc00);
            i += 1;
          }
        }
        if (cp < 0x80) out.push(cp);
        else if (cp < 0x800) out.push(0xc0 | (cp >> 6), 0x80 | (cp & 0x3f));
        else if (cp < 0x10000) out.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
        else out.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3f), 0x80 | ((cp >> 6) & 0x3f), 0x80 | (cp & 0x3f));
      }
      return new Uint8Array(out);
    }
    encodeInto(text, view) {
      const bytes = this.encode(text);
      const written = Math.min(bytes.length, view.length);
      view.set(bytes.subarray(0, written));
      return { read: text.length, written };
    }
  };
}
