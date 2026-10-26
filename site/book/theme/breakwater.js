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
        // Dark nodes with white text in the logo's blues, readable at WCAG AA contrast.
        window.mermaid.initialize({
            startOnLoad: false,
            theme: "base",
            themeVariables: {
                darkMode: true,
                background: "#161923",
                primaryColor: "#0b2a4a",
                primaryTextColor: "#ffffff",
                primaryBorderColor: "#00fbfc",
                secondaryColor: "#1f2433",
                tertiaryColor: "#1f2433",
                lineColor: "#b9b9b9",
                textColor: "#ffffff",
                clusterBkg: "#11141c",
                clusterBorder: "#818181",
                edgeLabelBackground: "#161923"
            }
        });
        window.mermaid.run();
    };
    document.head.appendChild(script);
})();
