export type GlassSettings = { blur: number; opacity: number };

export const DEFAULT_SETTINGS: GlassSettings = { blur: 16, opacity: 72 };

const SETTINGS_KEY = "trex-appearance";
const DB_NAME = "trex";
const STORE = "appearance";
const BACKGROUND_KEY = "background";

export function loadSettings(): GlassSettings {
  try {
    const parsed = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "null");
    if (typeof parsed?.blur === "number" && typeof parsed?.opacity === "number") return parsed;
  } catch (error) {
    console.warn("could not read appearance settings", error);
  }
  return DEFAULT_SETTINGS;
}

export function saveSettings(settings: GlassSettings) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch (error) {
    console.warn("could not save appearance settings", error);
  }
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function withStore<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await openDb();
  try {
    return await new Promise((resolve, reject) => {
      const request = run(db.transaction(STORE, mode).objectStore(STORE));
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
  } finally {
    db.close();
  }
}

export async function loadBackground(): Promise<Blob | null> {
  const value = await withStore("readonly", (store) => store.get(BACKGROUND_KEY));
  return value instanceof Blob ? value : null;
}

export function saveBackground(image: Blob) {
  return withStore("readwrite", (store) => store.put(image, BACKGROUND_KEY));
}

export function clearBackground() {
  return withStore("readwrite", (store) => store.delete(BACKGROUND_KEY));
}
