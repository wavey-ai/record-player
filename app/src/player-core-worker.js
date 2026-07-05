let engine = null;

async function initialise(moduleUrl) {
  const module = await import(moduleUrl);
  await module.default();
  engine = new module.WasmPlayerEngine({ sample_rate: 48000, sharp_crossfader_width: 0.08 });
  return snapshot();
}

function snapshot() {
  return {
    commands: engine.drainCommands(),
    view: engine.view(),
    revision: engine.revision()
  };
}

self.onmessage = async event => {
  const { id, type, payload } = event.data;
  try {
    let result;
    if (type === "init") {
      result = await initialise(payload.moduleUrl);
    } else if (type === "dispatch") {
      if (!engine) throw new Error("Player core is not initialised");
      engine.dispatch(payload.event);
      result = snapshot();
    } else if (type === "state") {
      if (!engine) throw new Error("Player core is not initialised");
      result = snapshot();
    } else {
      throw new Error(`Unknown player core request: ${type}`);
    }
    self.postMessage({ id, ok: true, result });
  } catch (error) {
    self.postMessage({ id, ok: false, error: error instanceof Error ? error.message : String(error) });
  }
};
