# Browser assets

The books use mdBook's native HTML theme. These browser-only bundles extend code-fenced Mermaid diagrams with an accessible PhotoSwipe image viewer; no JavaScript build tool is required.

| Dependency | Version | Source | License |
| --- | --- | --- | --- |
| Mermaid | 11.17.2 | https://github.com/mermaid-js/mermaid | MIT, `vendor/MERMAID-LICENSE` |
| PhotoSwipe | 5.4.4 | https://github.com/dimsemenov/PhotoSwipe | MIT, `vendor/PHOTOSWIPE-LICENSE` |

`diagrams.js` is the shared browser adapter. Mermaid renders with `securityLevel: strict` and HTML labels disabled. PhotoSwipe handles image zoom, pan, focus and keyboard navigation. Source blocks remain available when rendering fails. SVG exports contain the selected book theme's diagram colors.

To update dependencies, replace the corresponding official distribution bundle and license, update this table, build both books, and verify rendering, theme changes, zoom/pan, keyboard dismissal and SVG download in a browser. Do not edit minified vendor code.
