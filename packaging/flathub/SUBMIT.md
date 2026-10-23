# Skicka in LuxCanvas till Flathub

Filerna här (`io.github.dio99.LuxCanvas.yml` och `cargo-sources.json`) är det
som ska in i Flathub. Manifestet bygger från en tagg på GitHub, inte från den
lokala mappen.

## Första gången

1. Forka https://github.com/flathub/flathub. **Avmarkera "Copy the `master`
   branch only"** när GitHub frågar, så att grenen `new-pr` följer med.
2. Klona forken och skapa en gren från `new-pr`:

   ```bash
   git clone --branch new-pr git@github.com:dio99/flathub.git
   cd flathub
   git checkout -b luxcanvas
   cp ~/git/lumamap/packaging/flathub/io.github.dio99.LuxCanvas.yml .
   cp ~/git/lumamap/packaging/flathub/cargo-sources.json .
   git add . && git commit -m "Add io.github.dio99.LuxCanvas"
   git push origin luxcanvas
   ```

3. Öppna en pull request mot **`new-pr`** i flathub/flathub (inte `master`)
   med titeln `Add io.github.dio99.LuxCanvas`. Fyll i checklistan i mallen.
4. En bot bygger appen. Svara på frågor från granskarna i PR:en. När den
   godkänns skapas repot `flathub/io.github.dio99.LuxCanvas`, och du får
   skrivrätt till det.

Bra att veta inför granskningen:

- **`--device=all`** behövs för webbkameror (V4L2) och MIDI-kontroller
  (`/dev/snd/midi*`), som saknar portaler. Säg det om de frågar.
- **App-id `io.github.dio99.LuxCanvas`** matchar repot github.com/dio99/luxcanvas.
  Flathub kan be dig verifiera att du äger det (via flathub.org när appen är
  publicerad).

## Nya versioner

1. Tagga en ny version i luxcanvas-repot (t.ex. `v1.7.0`) och lägg till den
   under `<releases>` i `packaging/linux/io.github.dio99.LuxCanvas.metainfo.xml`
   (och flytta skärmdumparnas länkar till den nya taggen).
2. Har `Cargo.lock` ändrats: `python3 packaging/flatpak/cargo-sources.py` och
   kopiera `packaging/flatpak/cargo-sources.json` hit.
3. Uppdatera `tag` och `commit` i manifestet här och i
   flathub/io.github.dio99.LuxCanvas, och öppna en PR där.

## Provbygga som Flathub

```bash
cd packaging/flathub
flatpak run org.flatpak.Builder --user --install-deps-from=flathub --force-clean \
    --repo=/tmp/fh-repo /tmp/fh-build io.github.dio99.LuxCanvas.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo /tmp/fh-repo
```
