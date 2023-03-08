import { Provider } from "jotai";
function MyApp({ Component, pageProps }) {
  return /*#__PURE__*/ React.createElement(
    Provider,
    {
      __source: {
        fileName: "input.js",
        lineNumber: 5,
        columnNumber: 5,
      },
      __self: this,
    },
    /*#__PURE__*/ React.createElement(
      Component,
      _extends({}, pageProps, {
        __source: {
          fileName: "input.js",
          lineNumber: 6,
          columnNumber: 7,
        },
        __self: this,
      })
    )
  );
}
_c = MyApp;
export default MyApp;
var _c;
$___refreshReg$(_c, "MyApp");
