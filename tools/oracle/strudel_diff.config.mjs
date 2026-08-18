// Vitest config for `strudel_diff.test.mjs` — see the README's "Differential
// run" section. `root` is the vendored strudel checkout so its packages resolve.
import { defineConfig } from 'vitest/config';
import bundleAudioWorkletPlugin from '../../strudel/packages/vite-plugin-bundle-audioworklet/vite-plugin-bundle-audioworklet.js';

export default defineConfig({
  root: new URL('../../strudel/', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1'),
  plugins: [bundleAudioWorkletPlugin()],
  test: {
    isolate: false,
    setupFiles: './vitest.setup.mjs',
    // superdough imports its audioworklets extensionless, which only resolves
    // once vite transforms the package instead of handing it to node.
    server: { deps: { inline: [/superdough/, /supradough/, /@strudel/, /kabelsalat/] } },
    testTimeout: 3_600_000,
    include: [new URL('strudel_diff.test.mjs', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')],
  },
});
