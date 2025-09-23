globalThis.jotaiAtomCache = globalThis.jotaiAtomCache || {
    cache: new Map(),
    get (name, inst) {
        if (this.cache.has(name)) {
            return this.cache.get(name);
        }
        this.cache.set(name, inst);
        return inst;
    }
};
import { atom, useAtom } from "jotai";
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));
export default function AboutPage() {
    const [count, setCount] = useAtom(countAtom);
    return <div>
      <div>About us</div>
      {count} <button onClick={()=>setCount((c)=>c + 1)}>+1</button>
    </div>;
}
