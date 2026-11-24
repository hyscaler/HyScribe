// SPDX-License-Identifier: AGPL-3.0-or-later
// Run by build.sh in its Node container: each scheme's four files, with
// core's banner; Coffee must come out as the release ships it.
const fs = require("fs");
const sass = require("sass");
const rtlcss = require("rtlcss");
const CleanCSS = require("clean-css");

const banner = "/*! This file is auto-generated */\n";
for (const scheme of ["coffee", "hyscribe"]) {
	const dir = `colors/${scheme}/`;
	const css = sass.compile(`${dir}colors.scss`, { style: "expanded" }).css;
	const rtl = rtlcss.process(css);
	const files = {
		"colors.css": banner + css,
		"colors-rtl.css": banner + rtl,
		"colors.min.css": banner + new CleanCSS({}).minify(css).styles,
		"colors-rtl.min.css": banner + new CleanCSS({}).minify(rtl).styles,
	};
	for (const [name, text] of Object.entries(files)) {
		if (scheme === "coffee" && fs.readFileSync(dir + name, "utf8") !== text) {
			console.error(`core's Coffee differs: ${name}`);
			process.exit(1);
		}
		fs.writeFileSync(dir + name, text);
	}
}
