// The one thing on the site that needs a script: the download buttons point straight at the latest
// release's files, with their sizes. Without the script (or if GitHub does not answer) they open the
// release page.
(() => {
  const mb = (n) => `${(n / 1048576).toFixed(1)} MB`;
  fetch('https://api.github.com/repos/Zaious/ownvoice-meet/releases/latest', { headers: { accept: 'application/vnd.github+json' } })
    .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
    .then((rel) => {
      for (const a of document.querySelectorAll('a[data-file]')) {
        const f = (rel.assets ?? []).find((x) => x.name.endsWith(a.dataset.file));
        if (!f) continue;
        a.href = f.browser_download_url;
        const size = a.querySelector('.size');
        if (size) size.textContent = mb(f.size);
      }
      const ver = document.querySelector('.ver b');
      if (ver && rel.tag_name) {
        ver.textContent = rel.tag_name;
        ver.parentElement.hidden = false;
      }
    })
    .catch(() => { /* the buttons keep pointing at the release page */ });
})();
