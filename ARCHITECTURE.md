# LumaMap – Arkitektur

Videomappning för Linux, i samma anda som **MapMap** (enkelt, visuellt) och
**Splash** (flera projektorer, kantblandning). Fokus: *det ska gå att mappa en
video på en vägg inom två minuter utan att läsa en manual.*

---

## 1. Mål och icke-mål

**Mål (v1)**
- Spela video/bild/kamera och projicera på godtyckliga ytor (quad, triangel, mesh).
- Dra hörn direkt med musen – det man ser i editorn är det projektorn visar.
- Flera utgångar (projektorer) med kantblandning (edge blending).
- Spara/öppna projekt som en enda läsbar fil.
- Fjärrstyrning via OSC.
- Stabil 60 fps på vanlig laptop med integrerad grafik.

**Icke-mål (v1)**
- Automatisk kalibrering med kamera (Splash har det – kommer senare).
- 3D-modellbaserad mappning (Splash-stil `.obj`) – senare.
- Tidslinje/sekvenserare – bara enkla cue-listor i v1.
- Windows/macOS-stöd (fungerar troligen, men testas inte).

---

## 2. Teknikval

| Del              | Val                         | Varför |
|------------------|-----------------------------|--------|
| Språk            | **Rust**                    | Säker samtidighet (video-trådar + render), en binär, enkel bygg med `cargo`. |
| GPU              | **wgpu** (Vulkan/GL-backend)| Modernt, fungerar på Intel/AMD/NVIDIA, faller tillbaka till GL. |
| Fönster/skärmar  | **winit**                   | Flera fönster, välj skärm, fullskärm per projektor. |
| UI               | **egui** (`egui-wgpu`)      | Snabbt att bygga, ritar i samma GPU-kontext som mappningen, inget Qt-beroende. |
| Videoavkodning   | **GStreamer** (`gstreamer-rs`) | Standard på Linux, hårdvaruavkodning (VA-API/NVDEC), kamera via v4l2, NDI/RTSP via plugins. |
| Projektfil       | **RON** (`serde`)           | Läsbar, diffbar, versionerad. |
| Fjärrstyrning    | **OSC** (`rosc`) över UDP   | Samma som MapMap/Splash – funkar med TouchOSC, QLab, Ableton. |
| Paketering       | AppImage + Flatpak          | Ett klick att installera på alla distar. |

---

## 3. Användarupplevelse (styr allt annat)

Enkelheten är ett krav, inte en bonus. Arkitekturen ska göra följande möjligt:

```
 ┌───────────────────────────────────────────────────────────────────────┐
 │ [▶ Visa]  [Lägg till media]  [+ Yta ▾]   Utgång: [Projektor 1 ▾]  ⚙   │
 ├──────────────┬───────────────────────────────────────┬────────────────┤
 │ MEDIA        │                                       │ EGENSKAPER     │
 │ ▸ intro.mp4  │        UTGÅNGSVY (WYSIWYG)            │ Yta: Vägg A    │
 │ ▸ logo.png   │    ●────────────────●                 │ Källa: intro   │
 │ ▸ Kamera 0   │     \   video      /                  │ Opacitet ▬▬▬○  │
 │              │      ●────────────●                   │ Blend: Normal  │
 │ YTOR         │                                       │ Mask: ingen    │
 │ ☑ Vägg A     │                                       │ [Testbild]     │
 │ ☑ Pelare     │                                       │                │
 └──────────────┴───────────────────────────────────────┴────────────────┘
```

Principer:
1. **Dra-och-släpp**: släpp en videofil på ytan → klart. Ingen "skapa källa först".
2. **Live överallt**: varje ändring syns direkt på projektorn, inget "applicera".
3. **Två lägen**: *Redigera* (handtag och rutnät syns på projektorn) och *Visa* (rent).
4. **Ångra allt** (Ctrl+Z) – varje handling är ett kommando.
5. **Testbild** med ett klick per yta och per utgång (rutnät, färger, ID-nummer).
6. **Tangentbordsfinjustering**: piltangent flyttar valt hörn 1 px, Shift = 10 px.
7. **Autospara** var 30:e sekund + återställning efter krasch.

---

## 4. Övergripande struktur

```
                 ┌──────────────────────── Huvudtråd ───────────────────────┐
                 │                                                          │
 Mus/Tangentbord │  ┌────────┐  Command  ┌───────────┐  läser   ┌────────┐  │
 ───────────────▶│  │ Editor │──────────▶│  Project  │◀─────────│Renderer│──┼──▶ Editorfönster
                 │  │  (egui)│           │  (modell) │          │ (wgpu) │──┼──▶ Projektor 1..N
 OSC (UDP) ─────▶│  └────────┘  ▲        │ + Undo    │          └───▲────┘  │    (fullskärm)
   via kanal     │              │        └───────────┘              │       │
                 └──────────────┼───────────────────────────────────┼───────┘
                                │ Command                           │ senaste bildruta
                        ┌───────┴──────┐                    ┌───────┴────────┐
                        │  OSC-tråd    │                    │ Media-trådar   │
                        │  (rosc)      │                    │ (GStreamer,    │
                        └──────────────┘                    │  1 per källa)  │
                                                            └────────────────┘
```

- **En process, en GPU-enhet**, ett `wgpu::Device` delat av alla fönster.
- **Huvudtråden** äger projektmodellen, UI och rendering. Inga lås på modellen.
- **Media-trådar** avkodar och lämnar bara "senaste bildruta" i en trippelbuffer
  (aldrig kö → ingen fördröjning som växer).
- **Allt som ändrar projektet** (UI, OSC, tangentbord) går via `Command` → ger
  ångra/gör om och fjärrstyrning gratis.

---

## 5. Moduler (Cargo workspace)

```
lumamap/
├── Cargo.toml                 (workspace)
├── crates/
│   ├── lm-core/               Datamodell, Command, Undo, serialisering. Ingen GPU.
│   ├── lm-geom/               Homografi, mesh-warp, punkt-i-polygon, hit-test.
│   ├── lm-media/              GStreamer-källor, bild, färg, testmönster.
│   ├── lm-render/             wgpu-pipeline, shaders, utgångar, edge blend.
│   ├── lm-control/            OSC-server (MIDI senare).
│   └── lm-app/                main(): fönster, egui-editor, limmar ihop allt.
├── shaders/                   WGSL
├── assets/                    Ikoner, testbilder
└── examples/                  Exempelprojekt (.lmap)
```

Beroenderiktning (inga cykler):
`lm-app → lm-render, lm-media, lm-control → lm-core, lm-geom`

`lm-core` och `lm-geom` är rena och enhetstestbara utan GPU eller video.

---

## 6. Datamodell (`lm-core`)

```rust
struct Project {
    version: u32,
    sources:  Vec<Source>,     // Vad som visas
    surfaces: Vec<Surface>,    // Var det visas (ordning = lagerordning)
    outputs:  Vec<Output>,     // Vilken projektor
    cues:     Vec<Cue>,        // Sparade tillstånd att växla mellan
}

struct Source {
    id: SourceId,
    name: String,
    kind: SourceKind,          // Video{path, loop, speed} | Image{path}
                               // | Camera{device} | Color{rgba} | TestPattern
                               // | Stream{uri}  (RTSP/NDI/SRT via GStreamer)
}

struct Surface {
    id: SurfaceId,
    name: String,
    source: Option<SourceId>,
    output: OutputId,
    shape: Shape,              // Quad | Triangle | Mesh{cols, rows} | Ellipse
    src_pts: Vec<Vec2>,        // Utsnitt ur källan (0..1, normaliserat)
    dst_pts: Vec<Vec2>,        // Placering på utgången (0..1, normaliserat)
    opacity: f32,
    blend: BlendMode,          // Normal | Add | Multiply | Screen
    mask: Option<Mask>,        // Polygon eller bild, med mjuk kant (feather)
    color: ColorAdjust,        // Ljusstyrka, kontrast, gamma, färgton
    visible: bool,
    locked: bool,
}

struct Output {
    id: OutputId,
    name: String,
    monitor: MonitorHint,      // Namn + position, så rätt projektor hittas igen
    resolution: UVec2,
    edge_blend: EdgeBlend,     // Bredd + gamma per kant (V/H/Ö/N)
    keystone: [Vec2; 4],       // Global hörnkorrigering för hela utgången
}
```

Viktiga beslut:
- **Normaliserade koordinater (0..1)** överallt → projektet överlever byte av
  upplösning eller projektor.
- **ID:n, inte index** → stabila referenser i undo-historik och OSC-adresser.
- **`MonitorHint`**: hittar skärm via EDID-namn först, sedan position. Saknas
  projektorn öppnas utgången som vanligt fönster i stället för att krascha.

### Command & Undo

```rust
enum Command {
    MovePoint { surface: SurfaceId, which: PointRef, to: Vec2 },
    AddSurface(Surface), RemoveSurface(SurfaceId),
    SetSource { surface: SurfaceId, source: Option<SourceId> },
    SetProperty { target: Target, prop: Prop, value: Value },
    Transport { source: SourceId, action: Play | Pause | Seek(f64) },
    ...
}
```
- `apply(&mut Project) -> Command` returnerar sitt eget inverterade kommando.
- Kontinuerliga drag slås ihop till *ett* undo-steg (samma mål inom en gest).
- Transport (play/paus) hamnar inte i undo-historiken.

---

## 7. Media (`lm-media`)

```
 fil/kamera/ström ─▶ GStreamer pipeline ─▶ appsink (RGBA eller NV12)
                     uridecodebin                │
                     ! videoconvert              ▼
                     ! appsink           Trippelbuffer (senaste ruta)
                                                 │  huvudtråd: try_take()
                                                 ▼
                                         queue.write_texture()
```

- En `MediaSource`-trait: `fn latest_frame(&self) -> Option<Frame>`,
  `play/pause/seek/set_loop/set_speed`, `size()`.
- **NV12 → RGB i shader** för att halvera uppladdningen (viktigt för 4K).
- Bild-källor laddas en gång (`image`-crate), testmönster genereras på GPU.
- **Delad källa**: samma video på 5 ytor avkodas *en* gång.
- Ljud: GStreamer spelar upp ljudet från videokällan direkt (`autoaudiosink`),
  kan stängas av per källa.
- *Senare*: zero-copy via DMA-BUF från VA-API → Vulkan (stort prestandalyft).

---

## 8. Rendering (`lm-render`)

Per bildruta, per utgång:

```
 för varje yta (i lagerordning) på denna utgång:
     sampla källtextur genom src_pts  ──▶  warpa till dst_pts  ──▶  mask
     ──▶  färgjustering  ──▶  blanda in i utgångens bildbuffer
 sedan:  edge blend-ramp  ──▶  utgångs-keystone  ──▶  (redigeringsläge: handtag)
         ──▶  presentera på projektorns fönster (vsync)
```

### Geometri (`lm-geom`)
- **Quad**: beräkna 3×3-**homografi** från 4 punkter (DLT). Använd den i
  fragment-shadern (projektiv texturering) så att bilden blir perspektivriktig
  *utan* den synliga diagonala sömmen man får av två trianglar med affin UV.
- **Mesh**: rutnät `cols × rows` kontrollpunkter, bilinjär (senare Bézier)
  interpolation, tesselleras till många trianglar → för böjda ytor/pelare.
- **Triangel / Ellips**: specialfall som delar samma pipeline.
- Hit-test (vilket hörn/vilken yta är under musen) görs på CPU i `lm-geom`.

### Utgångar och fönster
- Varje `Output` = ett winit-fönster med egen `wgpu::Surface`, fullskärm på
  vald skärm. Editorns mittvy visar **samma** render-target (WYSIWYG).
- Varje utgång renderas först till en offscreen-textur → används både för
  projektorn och för förhandsvisning i editorn (ingen dubbelrendering).

### Edge blending (Splash-funktionen)
- Per kant: bredd (0..0.5) och gamma. Utförs som ett sista helskärmspass som
  multiplicerar med en ramp `pow(smoothstep(x), gamma)`.
- Svartnivåkompensation (black level) i v2.

---

## 9. Fjärrstyrning (`lm-control`)

OSC på UDP-port **12345** (konfigurerbar). Adresser byggs från namn/ID:

```
/lumamap/source/<namn>/play
/lumamap/source/<namn>/pause
/lumamap/source/<namn>/seek        f   (sekunder)
/lumamap/surface/<namn>/opacity    f   (0..1)
/lumamap/surface/<namn>/visible    i
/lumamap/cue/<nummer>/go
/lumamap/master/opacity            f   (blackout = 0)
```

OSC-tråden översätter meddelanden till `Command` och skickar via kanal till
huvudtråden – exakt samma väg som musen.

---

## 10. Projektfil

- Ett projekt = en `.lmap`-fil (RON), mediesökvägar **relativa** till filen.
- "Samla projekt" kopierar alla medier till en mapp → lätt att flytta till
  showdatorn.
- Fältet `version` + migreringsfunktioner så gamla projekt alltid går att öppna.
- `lumamap --play show.lmap` startar direkt i Visa-läge i fullskärm
  (för installationer som ska starta vid boot).

---

## 11. Prestanda och robusthet

| Krav | Hur |
|------|-----|
| 60 fps, låg latens | Ingen kö mellan avkodare och renderer, bara senaste ruta. |
| Ingen frysning i UI | Filöppning/avkodning i bakgrundstrådar. |
| Källa går sönder | Ytan visar svart/testmönster + varning i UI, resten fortsätter. |
| Projektor kopplas ur | Utgångsfönstret flyttas/öppnas igen när skärmen dyker upp. |
| Krasch | Autosparad kopia erbjuds vid nästa start. |
| Lång drift (installationer) | Inga minnesläckor: testas med 24 h-loop i CI-natt. |

---

## 12. Färdplan

| Version | Innehåll |
|---------|----------|
| **v0.1 – MVP** | Ett editorfönster + en utgång, quad-ytor, video/bild, dra hörn, ångra, spara/öppna, Visa-läge, testbild. |
| **v0.2** | Flera utgångar/projektorer, mesh-warp, masker, triangel/ellips, blend-lägen. |
| **v0.3** | Edge blending, OSC, cue-lista, kamera- och strömkällor, `--play`-läge. |
| **v0.4** | AppImage/Flatpak, NV12-shader, zero-copy, MIDI. |
| **v1.0** | Stabilitet, dokumentation, exempelprojekt. |
| Senare | Kamerakalibrering och 3D-modellmappning (Splash-nivå), shader-effekter, Syphon/Spout-liknande delning via PipeWire. |

---

## 13. Systemberoenden (Linux Mint / Ubuntu)

```bash
sudo apt install libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-libav \
    gstreamer1.0-vaapi libxkbcommon-dev libwayland-dev
```

---

## 14. Öppna frågor

1. **egui vs. Qt**: egui väljs för enkelhet och en GPU-kontext. Om UI:t behöver
   mer "desktop-känsla" (dockbara paneler) används `egui_dock`.
2. **Video-backend**: GStreamer valt. Alternativet `ffmpeg-next` ger mer
   kontroll men sämre stöd för kameror/strömmar och hårdvaruavkodning på Linux.
3. **Namn**: "LumaMap" är arbetsnamn.
