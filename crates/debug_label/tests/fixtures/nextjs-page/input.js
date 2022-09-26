import { atom, useAtom } from "jotai";

const countAtom = atom(0);

export default function AboutPage() {
  const [count, setCount] = useAtom(countAtom);
  return (
    <div>
      <div>About us</div>
      {count} <button onClick={() => setCount((c) => c + 1)}>+1</button>
    </div>
  );
}
