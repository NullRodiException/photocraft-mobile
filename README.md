# PhotoCraft Mobile

O editor do [PhotoCraft](../photocraft) com um **seletor rápido de modos** (Vetor | Pixel | Foto),
no estilo dos estúdios do Affinity. Este é o ponto de partida da versão Android.

O PhotoCraft é usado **como dependência por caminho** (`../photocraft/crates/*`) e nunca é
modificado daqui. Tudo o que o seletor faz passa pela superfície pública dele (comandos e
estado da UI).

## O que um modo faz

| Modo | Workspace (Window › Workspace) | Ferramentas | Ferramenta inicial |
|---|---|---|---|
| **Vetor** | Graphic and Web | caneta, seleção de caminho, formas, texto, artboard, slices | Caneta |
| **Pixel** | Painting | seleções, pincéis, borrachas, gradiente, balde, carimbo, borrar | Pincel |
| **Foto** | Photography | corte, Remover, Pincel de Recuperação, Correção, olhos vermelhos, Subexposição/Superexposição | Pincel de Recuperação para Manchas |

Mover, Mão, Zoom, Girar Vista e Conta-gotas ficam em todos os modos. Cada modo lembra a última
ferramenta usada nele, e o modo atual é salvo entre sessões.

Para trocar de modo: clique no seletor no topo, ou use **Alt+1 / Alt+2 / Alt+3**. Em telas
estreitas (menos de 700 pt, como o tablet em retrato), o seletor mostra só os ícones.

## Barra de ações

À direita do seletor ficam botões grandes para quem não tem teclado:

| Botão | O que faz |
|---|---|
| Desfazer / Refazer | `edit.undo` / `edit.redo` |
| Encaixar na tela | `view.fitOnScreen` |
| Foco | esconde e mostra a barra de ferramentas, a barra de opções e o dock (o Tab do Photoshop) |
| Dedo (só no Android) | alterna entre "dedos navegam" (padrão) e "dedos desenham"; a escolha é salva |

Ao entrar num modo:
1. `window.workspace.*` abre os painéis do modo;
2. `edit.toolbar {hidden}` esconde da barra de ferramentas o que o modo não usa;
3. a ferramenta lembrada do modo fica ativa.

## Estrutura

```text
crates/modes   lógica pura dos modos (sem UI, sem PhotoCraft): listas de ferramentas,
               workspace, memória da última ferramenta. Compila para qualquer alvo.
crates/shell   ModeShell: embrulha o PhotocraftApp e desenha o seletor e a barra de ações (egui).
               touch.rs: gestos de dedo no canvas (pan, pinça, desfazer/refazer).
               services.rs: serviços comuns (codecs, gravação atômica, configurações).
               examples/snapshot.rs renderiza offscreen um PNG por modo.
apps/desktop   executável `photocraft-mobile` (eframe/wgpu) para desenvolver no PC.
apps/android   app Android: biblioteca nativa + MainActivity.java (GameActivity, Gradle).
xtask          `cargo xtask android`: compila, empacota, assina, instala e abre o APK;
               `cargo xtask icons`: gera o ícone do launcher.
```

As configurações ficam em `%APPDATA%\PhotoCraftMobile` (ou `PHOTOCRAFT_MOBILE_CONFIG_DIR`), não
nas do PhotoCraft, porque trocar de modo reescreve a barra de ferramentas.

## Comandos

```sh
cargo run --release -p photocraft-mobile                 # o app
cargo run --release -p photocraft-mobile -- foto.jpg
cargo test                                               # todos os testes
cargo clippy --all-targets -- -D warnings
cargo run --release -p photocraft-mobile-shell --example snapshot -- --out shots --size 1280x800
```

O `Cargo.lock` começou como cópia do lock do PhotoCraft, para usar as mesmas versões testadas.
Ao atualizar o checkout do PhotoCraft, copie o lock de novo se o build reclamar de versões.

## Regras (herdadas do PhotoCraft)

- Nada de `unwrap`/`expect`/`panic!` fora de testes (`#![deny(...)]` em cada crate).
- Sem `unsafe` (`unsafe_code = "forbid"`), com uma única exceção isolada: o `android_main` (veja
  a seção Android).
- Rust puro, sem webview.

## Android

`apps/android` é um `cdylib` (`libphotocraft_mobile.so`) carregado pela `GameActivity` do
AndroidX, sem nenhum código Java nosso. Ele roda o mesmo `ModeShell` em eframe/wgpu (Vulkan, ou
GLES 3 quando não há Vulkan). Preferências e o modo salvo ficam no armazenamento interno do app.

O motivo de ser `GameActivity` e não `NativeActivity`: o PhotoCraft liga a feature `accesskit` do
eframe, e no Android ela só funciona com `GameActivity`. Ela também traz suporte ao teclado
virtual. Em troca, o empacotamento usa Gradle (para baixar a biblioteca AndroidX), e o
`android-activity` compila uma pequena camada de "cola" C++ do Google, a única parte não-Rust do app.

```sh
cargo xtask android                          # APK para arm64-v8a + x86_64 em target/android/
cargo xtask android --abi x86_64 --run       # só x86_64 (emulador), instala e abre
cargo xtask android --abi arm64-v8a --run    # celular real (depuração USB ligada)
cargo xtask android --logcat                 # depois de --run: mostra os logs do app
cargo xtask icons                            # regenera o ícone a partir do SVG do PhotoCraft
```

O ícone é o do PhotoCraft (a kitsune), como ícone adaptativo: fundo azul `#2F7BF5` e a arte na
camada da frente (`res/mipmap-*/ic_launcher_foreground.png`), que o launcher recorta no formato
dele. Os PNGs são gerados por `cargo xtask icons` a partir de
`../photocraft/assets/app-icon/photocraft.svg` e ficam versionados. Licença e atribuição em
`ATTRIBUTION.md`.

O `xtask` faz duas etapas. Primeiro, `cargo ndk` compila o `.so` (perfil `android-release`) para
`target/android/jniLibs/<abi>/`. Depois, o projeto Gradle em `apps/android` (`gradlew
assembleRelease`, AGP 8.7, `androidx.games:games-activity:4.4.0`) empacota tudo e assina com a
chave de debug. O APK final é copiado para `target/android/photocraft-mobile.apk`.

Requisitos:
- Android SDK com `platform-tools`, `build-tools;35.0.0`, `platforms;android-35` e um NDK
  (r28c testado), procurados em `ANDROID_HOME` ou `%LOCALAPPDATA%\Android\Sdk`.
- `JAVA_HOME` apontando para um JDK 17+.
- `cargo install cargo-ndk` e `rustup target add aarch64-linux-android x86_64-linux-android`.
- O Gradle vem no wrapper (`apps/android/gradlew`); não precisa instalar.

O único `unsafe` do projeto é a exportação do `android_main` (`#[unsafe(no_mangle)]`). Por isso o
crate Android usa `unsafe_code = "deny"` em vez de `forbid` e libera só esse item.

### Caneta (S Pen)

- **Pressão, inclinação e botão lateral** (`apps/android/src/pen.rs`): a `MainActivity` manda cada
  evento da caneta, encostando ou pairando (`nativePenSample`), com pressão, `AXIS_TILT`,
  orientação e botões.
  - Isso vai para o `StylusFeed` do PhotoCraft, o mesmo canal dos leitores de mesa digitalizadora
    do desktop. Pressão no tamanho e na opacidade e a curva de pressão em Preferências › Ferramentas
    funcionam como no desktop, e cada amostra leva a inclinação.
  - A conversão de tilt e orientação em tiltX/tiltY é a mesma do Chromium (`crates/shell/src/pen.rs`).
  - **Botão lateral da S Pen = borracha:** segurar o botão troca para a Borracha, e soltar volta para
    a ferramenta anterior. É o tratamento de "virar a caneta" que o PhotoCraft já tem.
  - A força que o winit põe no toque da caneta é descartada (`pen::input_hook`), para que pressão e
    inclinação venham da mesma amostra.
  - A rotação do corpo da caneta não existe na S Pen e fica em 0.
- **Contorno do pincel ao pairar:** os eventos de hover da caneta movem o ponteiro do egui, e o
  PhotoCraft desenha o contorno do pincel antes de a caneta encostar.
- **Rejeição de palma** (`MainActivity.java`): enquanto a caneta encosta ou paira sobre a tela, e
  por 0,5 s depois, toques que começam com dedo ou palma são descartados. Com a caneta longe, os
  dedos funcionam normalmente.
- **Tela cheia imersiva** (`MainActivity.java`): barra de status, barra de navegação e a taskbar
  do One UI ficam escondidas. Um deslize a partir da borda as mostra por um instante.
### Campos numéricos: segurar e deslizar

Nos campos de valor (Opacidade, Fluxo, Suavização, tamanho etc.), com o dedo ou a caneta:
- **segure** o campo por ~0,35 s: o campo é focado sem abrir o teclado, e aparece um slider
  vertical ao lado dele;
- **deslize para cima ou para baixo:** cada 6 pt é ±1. Afastando-se mais de 80 pt para o lado, cada
  passo vale ±10;
- **solte** para confirmar.

Arrastar direto, sem segurar antes, usa o ajuste rápido do próprio campo do PhotoCraft (cerca de
0,5 por ponto, também na vertical). Segurar antes dá o modo fino, com o slider. Um toque rápido
continua editando por texto. É o `ValueScrub` (`crates/shell/src/scrub.rs`): o
toque longo vira um clique, o deslizar vira setas ↑/↓ (que o campo do PhotoCraft já aceita) e
soltar vira Enter. As setas só são enviadas quando o foco caiu num campo de texto, porque fora
dele seriam atalhos (como mover a camada).

### Teclado na tela

O teclado só abre quando você toca num campo de texto. Diálogos que focam um campo sozinhos (como
Novo Documento) não o abrem mais por cima dos botões.

### Arquivos (seletor do Android)

Arquivo › Abrir e Salvar / Salvar Como / Exportar usam o seletor de arquivos do sistema (Storage
Access Framework, `apps/android/src/files.rs` + `MainActivity.java`):
- **Abrir:** os documentos escolhidos são copiados para o cache do app e abertos de lá.
- **Salvar:** o seletor cria o documento. O PhotoCraft grava num arquivo do cache ligado a ele, e
  a `MainActivity` copia para o documento escolhido, com um aviso "Salvo". Salvar de novo o mesmo
  arquivo vai direto para lá, sem abrir o seletor.
- Pedidos e respostas trafegam como linhas de texto pelo JNI (a `MainActivity` consulta os pedidos
  a cada 150 ms), sem nenhum bloco `unsafe`.
- Escolher pasta ainda não é suportado.

### Dedos: navegam, não pintam

Com a caneta, o dedo no canvas serve para navegar:

| Gesto no canvas | Ação |
|---|---|
| arrastar com 1 dedo | move a vista |
| 2 dedos | pinça (zoom em volta do centro dos dedos) e arrastar |
| toque rápido com 2 dedos | desfazer |
| toque rápido com 3 dedos | refazer |

Fora do canvas (menus, painéis, barra) o dedo funciona normalmente. O botão do dedo na barra
desliga isso para quem pinta com o dedo.

Como funciona:
- O winit descarta o tipo de ferramenta. Por isso a `MainActivity` informa ao Rust, para cada
  contato novo, se é dedo ou caneta (`nativeTouchTool`, em `apps/android/src/touch_tools.rs`).
- O `TouchRouter` (`crates/shell/src/touch.rs`) tira do fluxo de eventos o gesto dos dedos que
  começaram no canvas, junto com os cliques que o egui emula a partir deles, e devolve pan, zoom
  e toques.
- O pan move a vista ativa direto (`app.ui.views`, a mesma conta do canvas). O zoom vai ao
  PhotoCraft como `Event::Zoom`, que ele aplica com os próprios limites.

### Escala e espaço

- Na primeira execução, a escala da interface fica ~25% acima da do sistema (no Tab S9, 250%),
  para alvos de toque maiores. Dá para mudar em Preferências › Interface › Escala da UI.
- O trilho de ícones à direita do dock fica escondido, e o canvas ganha essa largura.
- **Memória:** na primeira execução, os limites ficam mais conservadores que os do desktop:
  - Uso de memória: um terço da RAM, entre 1 e 4 GB (`/proc/meminfo`);
  - Histórico: 30 estados;
  - Cache de efeitos: 256 MB.

  O Android encerra apps que usam memória demais. Tudo pode ser ajustado em Preferências › Desempenho.

O que ainda falta no Android:
- O botão Discord e a densidade dos menus e diálogos são do PhotoCraft e não mudam sem alterá-lo.
- Escolher pasta no seletor de arquivos.
