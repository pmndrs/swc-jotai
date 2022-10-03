var _s = $___refreshSig$();
import { atom, useAtom } from "jotai";
var countAtom = atom(0);
countAtom.debugLabel = "countAtom";
export default function AboutPage() {
    _s();
    var ref = _slicedToArray(useAtom(countAtom), 2), count = ref[0], setCount = ref[1];
    return /*#__PURE__*/ React.createElement("div", {
        __source: {
            fileName: "input.js",
            lineNumber: 8,
            columnNumber: 5
        },
        __self: this
    }, /*#__PURE__*/ React.createElement("div", {
        __source: {
            fileName: "input.js",
            lineNumber: 9,
            columnNumber: 7
        },
        __self: this
    }, "About us"), count, " ", /*#__PURE__*/ React.createElement("button", {
        onClick: function() {
            return setCount(function(c) {
                return c + 1;
            });
        },
        __source: {
            fileName: "input.js",
            lineNumber: 10,
            columnNumber: 15
        },
        __self: this
    }, "+1"));
}
_s(AboutPage, "sySahu1cSQLJWE74sAFuY2ik8VA=", false, function() {
    return [
        useAtom
    ];
});
_c = AboutPage;
var _c;
$___refreshReg$(_c, "AboutPage");
