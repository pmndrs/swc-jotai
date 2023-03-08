var _s = $___refreshSig$();
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
import { atom, useAtom } from "jotai";
const countAtom = globalThis.jotaiAtomCache.get("atoms.ts/countAtom", atom(0));
export default function AboutPage() {
  _s();
  const [count, setCount] = useAtom(countAtom);
  return /*#__PURE__*/ React.createElement(
    "div",
    {
      __source: {
        fileName: "input.js",
        lineNumber: 8,
        columnNumber: 5,
      },
      __self: this,
    },
    /*#__PURE__*/ React.createElement(
      "div",
      {
        __source: {
          fileName: "input.js",
          lineNumber: 9,
          columnNumber: 7,
        },
        __self: this,
      },
      "About us"
    ),
    count,
    " ",
    /*#__PURE__*/ React.createElement(
      "button",
      {
        onClick: () => setCount((c) => c + 1),
        __source: {
          fileName: "input.js",
          lineNumber: 10,
          columnNumber: 15,
        },
        __self: this,
      },
      "+1"
    )
  );
}
_s(AboutPage, "sySahu1cSQLJWE74sAFuY2ik8VA=", false, function () {
  return [useAtom];
});
_c = AboutPage;
var _c;
$___refreshReg$(_c, "AboutPage");
