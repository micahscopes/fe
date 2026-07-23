import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const main = readFileSync(new URL("./main.js", import.meta.url), "utf8");
const html = readFileSync(new URL("./index.html", import.meta.url), "utf8");

assert.match(main, /resolveQualityProfile\(qualityQuery, fixedResolution\)/);
assert.match(main, /qualityStatus\(activeQuality, canvas\.width, canvas\.height\)/);
assert.match(main, /quality=\$\{quality\.profile\} resolution=\$\{pixels\}×\$\{pixels\}/);
assert.match(html, /id="quality-values"/);
assert.match(html, /id="quality-teaser"/);
assert.match(html, /id="quality-full"/);
assert.match(html, /verify=off&amp;quality=teaser/);

console.log("CGA quality profile integration: ok");
