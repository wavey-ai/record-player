const DB_NAME = "vin.yl.player";
const DB_VERSION = 2;
const STORE_NAME = "scratch-performances";

function requestPromise(request) {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error("IndexedDB request failed"));
  });
}

function openDatabase() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);
    request.onupgradeneeded = () => {
      const database = request.result;
      if (!database.objectStoreNames.contains(STORE_NAME)) {
        const store = database.createObjectStore(STORE_NAME, { keyPath: "id" });
        store.createIndex("recordHash", "recordHash", { unique: false });
        store.createIndex("createdAt", "createdAt", { unique: false });
        store.createIndex("recordHashCreatedAt", ["recordHash", "createdAt"], { unique: false });
      }
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error || new Error("Unable to open scratch performance database"));
  });
}

async function withStore(mode, callback) {
  const database = await openDatabase();
  try {
    const transaction = database.transaction(STORE_NAME, mode);
    const result = await callback(transaction.objectStore(STORE_NAME));
    await new Promise((resolve, reject) => {
      transaction.oncomplete = resolve;
      transaction.onerror = () => reject(transaction.error || new Error("IndexedDB transaction failed"));
      transaction.onabort = () => reject(transaction.error || new Error("IndexedDB transaction aborted"));
    });
    return result;
  } finally {
    database.close();
  }
}

export async function saveScratchPerformance(performance) {
  await withStore("readwrite", store => requestPromise(store.put(structuredClone(performance))));
  return performance;
}

export async function getScratchPerformance(id) {
  return withStore("readonly", store => requestPromise(store.get(String(id))));
}

export async function listScratchPerformances({ recordHash = "", limit = 100 } = {}) {
  return withStore("readonly", async store => {
    const request = recordHash
      ? store.index("recordHash").getAll(String(recordHash))
      : store.getAll();
    const records = await requestPromise(request);
    return records
      .sort((a, b) => String(b.createdAt).localeCompare(String(a.createdAt)))
      .slice(0, Math.max(1, Number(limit) || 100));
  });
}

export async function deleteScratchPerformance(id) {
  await withStore("readwrite", store => requestPromise(store.delete(String(id))));
}

export async function clearScratchPerformances({ recordHash = "" } = {}) {
  if (!recordHash) {
    await withStore("readwrite", store => requestPromise(store.clear()));
    return;
  }
  const records = await listScratchPerformances({ recordHash, limit: Number.MAX_SAFE_INTEGER });
  await withStore("readwrite", store => Promise.all(records.map(record => requestPromise(store.delete(record.id)))));
}
