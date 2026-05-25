// Single-file bundler for the extension entry point. Keeps the published
// .vsix tiny by tree-shaking and dropping source maps in production.
const esbuild = require("esbuild");

const watch = process.argv.includes("--watch");
const prod = process.argv.includes("--production") || process.env.NODE_ENV === "production";

const ctx = esbuild.context({
  entryPoints: ["src/extension.ts"],
  bundle: true,
  outfile: "out/extension.js",
  platform: "node",
  target: "node18",
  external: ["vscode"],
  format: "cjs",
  sourcemap: !prod,
  minify: prod,
  logLevel: "info",
});

(async () => {
  const built = await ctx;
  if (watch) {
    await built.watch();
    console.log("[osh] watching…");
  } else {
    await built.rebuild();
    await built.dispose();
  }
})();
