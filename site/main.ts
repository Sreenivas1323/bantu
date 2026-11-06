// The only script on the page: copy buttons. Everything else is HTML and CSS.

for (const button of document.querySelectorAll<HTMLButtonElement>("button[data-copy]")) {
  let reset: ReturnType<typeof setTimeout> | undefined;
  button.addEventListener("click", async () => {
    const text = document.getElementById(button.dataset.copy ?? "")?.textContent ?? "";
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      return;
    }
    button.dataset.done = "";
    button.setAttribute("aria-label", "Copied");
    clearTimeout(reset);
    reset = setTimeout(() => {
      delete button.dataset.done;
      button.setAttribute("aria-label", "Copy the install command");
    }, 1600);
  });
}
