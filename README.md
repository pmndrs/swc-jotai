# swc-jotai

SWC plugins for [Jotai](https://github.com/pmndrs/jotai).

[Try it out using CodeSandbox](https://codesandbox.io/s/next-js-with-custom-swc-plugins-ygiuzm).

## Install

```sh
npm install --save-dev @swc-jotai/debug-label @swc-jotai/react-refresh
```

The plugins can be used by themselves as well.

## Usage

You can add the plugins to `.swcrc`:

Then update your `.swcrc` file like below:

```json
{
  "jsc": {
    "experimental": {
      "plugins": [
        ["@swc-jotai/debug-label", {}],
        ["@swc-jotai/react-refresh", {}]
      ]
    }
  }
}
```

You can use the plugins with [experimental SWC plugins feature](https://nextjs.org/docs/advanced-features/compiler#swc-plugins-experimental) in Next.js.

```js
module.exports = {
  experimental: {
    swcPlugins: [
      ["@swc-jotai/debug-label", {}],
      ["@swc-jotai/react-refresh", {}],
    ],
  },
};
```

### Custom atom names

You can enable the plugins for your custom atoms. You can supply them to the plugins like below:

```js
module.exports = {
  experimental: {
    swcPlugins: [
      ["@swc-jotai/debug-label", { atomNames: ["customAtom"] }],
      ["@swc-jotai/react-refresh", { atomNames: ["customAtom"] }],
    ],
  },
};
```
