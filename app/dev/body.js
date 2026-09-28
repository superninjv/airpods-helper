// DEV ONLY — pull the real markup out of ../src/index.html so the mock page
// never drifts from the app. Synchronous on purpose: main.js expects the DOM
// to exist when it runs. Needs an HTTP origin (file:// blocks the request):
//   python3 -m http.server -d app 8765  →  http://localhost:8765/dev/mock.html
"use strict";

(function () {
  const xhr = new XMLHttpRequest();
  xhr.open("GET", "../src/index.html", false);
  xhr.send();
  const doc = new DOMParser().parseFromString(xhr.responseText, "text/html");
  for (const node of [...doc.body.childNodes]) {
    if (node.nodeName !== "SCRIPT") document.body.append(document.adoptNode(node));
  }
})();
