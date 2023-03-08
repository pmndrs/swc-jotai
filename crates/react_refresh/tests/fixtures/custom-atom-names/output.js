globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
  cache: new Map(),
  get(name, inst) {
    if (this.cache.has(name)) {
      return this.cache.get(name);
    }
    this.cache.set(name, inst);
    return inst;
  },
};
const myCustomAtom = globalThis.jotaiAtomCache.get(
  "atoms.ts/myCustomAtom",
  customAtom(0)
);
