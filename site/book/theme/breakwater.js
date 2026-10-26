// Breakwater additions to the mdBook pages:
// - a link back to the landing page next to the GitHub button;
// - Mermaid diagrams, rendered from the same-origin copy of mermaid.min.js that
//   site/build.sh places next to the book, loaded only on pages that have a diagram.
(function () {
    "use strict";

    var buttons = document.querySelector(".right-buttons");
    if (buttons) {
        var home = document.createElement("a");
        home.href = "/";
        home.title = "Breakwater home";
        home.setAttribute("aria-label", "Breakwater home");
        home.className = "breakwater-home";
        home.textContent = "Home";
        buttons.insertBefore(home, buttons.firstChild);
    }

    var blocks = document.querySelectorAll("code.language-mermaid");
    if (!blocks.length) {
        return;
    }
    blocks.forEach(function (code) {
        var diagram = document.createElement("pre");
        diagram.className = "mermaid";
        diagram.textContent = code.textContent;
        code.parentElement.replaceWith(diagram);
    });
    var script = document.createElement("script");
    script.src = path_to_root + "mermaid.min.js";
    script.onload = function () {
        window.mermaid.initialize({ startOnLoad: false, theme: "dark" });
        window.mermaid.run();
    };
    document.head.appendChild(script);
})();
