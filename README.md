# PhotoCraft Mobile

O editor do [PhotoCraft](../photocraft) numa **interface minimalista no estilo do Procreate**, com
um **seletor rápido de modos** (Vetor | Pixel | Foto) no estilo dos estúdios do Affinity. Este é o
ponto de partida da versão Android.

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

Para trocar de modo: toque no seletor da barra superior, ou use **Alt+1 / Alt+2 / Alt+3**. Em
telas estreitas (menos de 760 pt, como o tablet em retrato), a barra mostra só os ícones.

## A interface (`crates/shell/src/studio.rs`)

A barra de menus, a barra de ferramentas, a barra de status, o dock e as abas de documento do
PhotoCraft ficam escondidos. No lugar deles:

**Barra superior**, da esquerda para a direita:

| Botão | O que faz |
|---|---|
| Galeria | mostra e esconde a tela inicial (arquivos recentes, novo, abrir) |
| Ações (engrenagem) | todos os menus do PhotoCraft em duas colunas (menus à esquerda, itens à direita), com busca; a categoria **Rápido** tem Novo, Abrir, Salvar, Exportar, Encaixar na tela, Tela cheia, "sempre mostrar a barra de opções", "dedos navegam", Sensibilidade da caneta e Configurações |
| Ajustes (varinha) | Imagem › Ajustes e o menu Filtro |
| Seleção (laço) | as ferramentas de seleção do modo e o menu Selecionar |
| Transformar (seta) | Transformação livre (`edit.freeTransform`); tocar de novo aplica |
| Vetor / Pixel / Foto | o seletor de modos |
| Todas as ferramentas (grade) | todas as ferramentas do modo; mostra o ícone da ferramenta ativa quando ela não é uma das três ao lado |
| 3 ferramentas do modo | Pixel: Pincel, Borrar, Borracha · Vetor: Caneta, Retângulo, Texto · Foto: Recuperação, Carimbo, Subexposição. Tocar de novo na ferramenta de pincel ativa abre a **Biblioteca de pincéis** (grupos à esquerda, traços de prévia à direita) |
| Camadas | abre e fecha o painel Camadas à direita |
| Círculo de cor | o popover de **Cores**: seletor em **quadrado** (anel de matiz + saturação/brilho) ou em **triângulo** (anel + triângulo matiz/branco/preto e barras H/S/V, como o Advanced Color Selector do Krita), escolhido no cabeçalho e salvo; cor de frente e fundo (toque troca), cores recentes e "Mais…" (o seletor completo do PhotoCraft) |

**Barra lateral** flutuando sobre o canvas: tamanho do pincel (escala logarítmica, 1 a 2500 px),
o botão **modificar** (conta-gotas; tocar de novo volta para a ferramenta anterior), opacidade
(do pincel, ou do preenchimento no Degradê e no Balde), **sensibilidade da caneta**, desfazer e
refazer. Enquanto um slider se move, um balão mostra o pincel no tamanho real na tela ou a
porcentagem.

**Sensibilidade da caneta** (o medidor fixo na barra lateral, ou Ações › Rápido): um painel ao
lado da barra com:
- "Usar a pressão da caneta" (Preferências › Ferramentas do PhotoCraft);
- a curva de pressão: os ajustes prontos Suave / Linear / Duro, um slider de Duro a Suave e o
  gráfico, onde se arrasta um ponto (as pontas só sobem e descem), toca-se num lugar vazio para
  criar um ponto e dá-se toque duplo para remover um ponto do meio;
- uma prévia do traço, do leve ao forte;
- com um pincel ativo, se a pressão muda o tamanho e a opacidade dele.

A curva é a mesma de Preferências › Ferramentas › Curva de pressão (`prefs.set`), gravada quando o
arraste termina, e o PhotoCraft a salva sozinho.

**Popovers:** um por vez, pendurado no seu botão. Tocar fora fecha sem pintar no canvas; Esc
também fecha.

**Barra de opções do PhotoCraft:** só aparece para ferramentas cujas opções a barra lateral não
cobre (seleções, formas, texto, corte, degradê, carimbo…) e durante uma transformação, edição de
texto ou corte (para Aplicar / Cancelar). Ações › Rápido › "Sempre mostrar a barra de opções" a
deixa fixa.

**Painéis:** Camadas abre o dock só com o painel Camadas. Um painel aberto por Ações › Janela
(Histórico, Navegador…) aparece no mesmo lugar.

**Tela cheia:** Ações › Rápido › Tela cheia deixa só a imagem e um botão no canto para voltar.

Ao entrar num modo:
1. `window.workspace.*` abre os painéis do modo;
2. `edit.toolbar {hidden}` esconde da barra de ferramentas o que o modo não usa;
3. a ferramenta lembrada do modo fica ativa.

## Estrutura

```text
crates/modes   lógica pura dos modos (sem UI, sem PhotoCraft): listas de ferramentas,
               workspace, memória da última ferramenta. Compila para qualquer alvo.
crates/shell   ModeShell: embrulha o PhotocraftApp na interface (egui).
               studio.rs: barra superior, barra lateral, popovers, seletores de cor.
               touch.rs: gestos de dedo no canvas (pan, pinça, desfazer/refazer).
               services.rs: serviços comuns (codecs, gravação atômica, configurações).
               examples/snapshot.rs renderiza offscreen um PNG por modo (e popovers).
               examples/frame_bench.rs mede o custo de CPU dos frames, sem janela.
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
cargo xtask ci                                           # antes de cada commit (abaixo)
cargo test                                               # todos os testes
cargo clippy --all-targets -- -D warnings
cargo run --release -p photocraft-mobile-shell --example snapshot -- --out shots --size 1280x800
# com um documento de exemplo e um popover aberto (actions, adjustments, selection, tools,
# brushes, color); --layers abre o painel Camadas e --triangle usa o seletor de cor em triângulo
cargo run --release -p photocraft-mobile-shell --example snapshot -- --out shots --new 1600x1200 --popover brushes
# custo de CPU por frame (média e p95, em ms) ocioso, com a caneta pairando, pintando, no pan
# com o dedo e com cada popover aberto; por padrão na tela do Tab S9 FE (920x576 pt a 2,5x).
# --only actions mede só um caso. Compare medições feitas na mesma máquina.
cargo run --release -p photocraft-mobile-shell --example frame_bench -- --frames 150
```

O `Cargo.lock` começou como cópia do lock do PhotoCraft, para usar as mesmas versões testadas.
Ao atualizar o checkout do PhotoCraft, copie o lock de novo se o build reclamar de versões.

### Verificação antes do commit e CI

`cargo xtask ci` roda, em ordem: `cargo fmt --check`, o Clippy com avisos como erro, os testes, o
Clippy do crate Android para arm64-v8a (pelo `cargo ndk`, porque fora do Android esse crate compila
vazio) e, por fim, diz se o `../photocraft` está no commit testado. `--no-android` pula o passo do
Android quando não há NDK.

A CI no GitHub (`.github/workflows/ci.yml`) roda o mesmo `cargo xtask ci` no Linux, com este
repositório e o `storytold/photocraft` lado a lado, o PhotoCraft no commit do `photocraft.rev`.

### A versão do PhotoCraft

O PhotoCraft entra por caminho, então o build usa o commit que estiver em `../photocraft`. O
arquivo `photocraft.rev` guarda o commit com que este projeto foi testado por último.

```sh
cargo xtask upstream            # o ../photocraft está no commit testado?
cargo xtask upstream --accept   # grava o commit atual do ../photocraft como testado
```

Para atualizar o PhotoCraft:
1. `git -C ../photocraft pull --ff-only upstream main`;
2. `cargo xtask ci` e conferir o app;
3. `cargo xtask upstream --accept` e um commit do `photocraft.rev` (com o `Cargo.lock`, se mudou).

Para voltar ao commit testado: `git -C ../photocraft checkout <commit do photocraft.rev>`.

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
chave de debug.

**Cada build é uma versão nova, num APK novo.** O número do último build fica em
`apps/android/build-number` (versionado). O `xtask` usa o próximo número como `versionCode` (e
`versionName` `0.1.<n>`), grava o APK em `target/android/photocraft-mobile-v<n>.apk` e só então
atualiza o arquivo. Os APKs anteriores continuam lá. Um build sem arm64-v8a (só o emulador) sai
como `photocraft-mobile-v<n>-x86_64.apk`. Como o `versionCode` sempre sobe, cada APK instala por
cima do anterior.

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

### Recuperação após falha

O Android pode encerrar um app em segundo plano a qualquer momento para liberar memória. Por isso
o app liga a recuperação do PhotoCraft (`services::recovery`, a mesma do app de desktop):
- os documentos com alterações não salvas são gravados em segundo plano em `recovery/`, dentro do
  armazenamento interno do app (no desktop, na pasta de configurações). Na instalação nova, isso
  acontece a cada 2 minutos (Preferências › Manipulação de arquivos; no desktop, 10);
- ao sair da tela (`onPause`), a `MainActivity` pede um autosave na hora. Depois que a
  `GameActivity` pausa, o winit não desenha mais frames, então ela espera até 250 ms um frame pegar
  o pedido (`apps/android/src/lifecycle.rs`, `PlatformEvent::Background`);
- na próxima abertura, o PhotoCraft devolve o que ficou lá. Salvar ou fechar o documento apaga os
  dados de recuperação dele.

### Teclado na tela

O teclado só abre quando você toca num campo de texto. Diálogos que focam um campo sozinhos (como
Novo Documento) não o abrem mais por cima dos botões.

### Arquivos (seletor do Android)

Arquivo › Abrir e Salvar / Salvar Como / Exportar usam o seletor de arquivos do sistema (Storage
Access Framework, `apps/android/src/files.rs` + `MainActivity.java`):
- **Abrir:** os documentos escolhidos são copiados para o cache do app e abertos de lá.
- **Salvar Como / Exportar:** o seletor cria o documento. O PhotoCraft grava num arquivo do cache
  ligado a ele, e a `MainActivity` copia para o documento escolhido, com um aviso "Salvo".
- **Cada cópia no cache fica ligada ao seu documento** (`crates/shell/src/saf.rs`). Por isso Salvar
  num documento aberto pelo seletor grava de volta no arquivo original, e salvar de novo um
  documento salvo vai direto para ele, sem abrir o seletor. Os vínculos ficam em
  `saf-links.json`, e a `MainActivity` guarda as permissões das URIs
  (`takePersistableUriPermission`), então isso continua valendo depois de o Android reiniciar o app.
- Um arquivo que o app só pode ler, ou uma cópia que perdeu o vínculo, recusa o Salvar com uma
  mensagem pedindo Salvar Como. Antes, a gravação ia só para a cópia no cache e parecia salva.
- **Limpeza do cache:** ao abrir o app, as pastas `cache/saf-open/<id>` e `cache/saf-save/<id>`
  que nenhum arquivo recente nem a recuperação usam são apagadas, junto com os vínculos delas. A
  `MainActivity` devolve as permissões de URI que mais nenhum vínculo usa. Por isso uma cópia
  continua no cache enquanto o arquivo estiver nos recentes do PhotoCraft.
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

Fora do canvas (menus, painéis, barras) o dedo funciona normalmente. Ações › Rápido › "Dedos
navegam" desliga isso para quem pinta com o dedo. Segurar e deslizar (abaixo) não vale nas barras
e popovers da interface, para que um toque lento num slider continue sendo um arraste.

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
- **GPU:** o PhotoCraft limita as texturas de um documento a um quarto da RAM, mas não sabe ler a
  RAM no Android e supõe 16 GB. A `MainActivity` informa o valor real em `PHOTOCRAFT_RAM_MB`
  (`ActivityManager`) antes de a biblioteca iniciar. Como no desktop e na web, o app pede os
  limites de textura da própria GPU (`gpu_canvas::use_adapter_limits`), e não o teto de 8192 px
  do egui: imagens maiores que isso continuam no compositor da GPU.

### Desempenho

- Os itens de menu dos popovers Ações, Ajustes e Seleção (estado ativado/marcado de cada item)
  são montados de novo só quando o documento, as ferramentas ou os painéis mudam, ou num frame
  com toque ou tecla, como a barra de menus do PhotoCraft faz. Montá-los a cada frame custava
  ~3 ms no desktop. A busca de Ações fica guardada junto, até a consulta mudar.
- A barra superior e as grades de ferramentas acham as ferramentas pelo nome numa tabela pronta
  (`tool_named`), e não com `Tool::from_name`, que monta várias strings por ferramenta a cada chamada.
- Segurar um campo para deslizar não pede frames enquanto o dedo fica parado.
- `frame_bench` (em Comandos) mede tudo isso.

O que ainda falta no Android:
- O botão Discord da tela inicial e a densidade dos menus e diálogos são do PhotoCraft e não mudam
  sem alterá-lo.
- Escolher pasta no seletor de arquivos.
