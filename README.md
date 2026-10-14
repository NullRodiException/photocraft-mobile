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

Para trocar de modo: clique no seletor no topo, ou use **Alt+1 / Alt+2 / Alt+3**.

Ao entrar num modo:
1. `window.workspace.*` abre os painéis do modo;
2. `edit.toolbar {hidden}` esconde da barra de ferramentas o que o modo não usa;
3. a ferramenta lembrada do modo fica ativa.

## Estrutura

```text
crates/modes   lógica pura dos modos (sem UI, sem PhotoCraft): listas de ferramentas,
               workspace, memória da última ferramenta. Compila para qualquer alvo.
crates/shell   ModeShell: embrulha o PhotocraftApp e desenha o seletor (egui).
               services.rs: serviços comuns (codecs, gravação atômica, configurações).
               examples/snapshot.rs renderiza offscreen um PNG por modo.
apps/desktop   executável `photocraft-mobile` (eframe/wgpu) para desenvolver no PC.
apps/android   biblioteca nativa do app Android (NativeActivity).
xtask          `cargo xtask android`: compila, empacota, assina, instala e abre o APK.
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

- **Pressão:** a GameActivity só copia X e Y de cada toque, a menos que outros eixos sejam
  ligados. O `android_main` liga `Axis::Pressure`, e a pressão chega ao PhotoCraft como a força do
  toque (`egui::Event::Touch { force }`), o mesmo caminho do Windows. Pressão no tamanho e na
  opacidade, e a curva de pressão em Preferências › Ferramentas, funcionam como no desktop.
- **Rejeição de palma** (`MainActivity.java`): enquanto a caneta encosta ou paira sobre a tela, e
  por 0,5 s depois, toques que começam com dedo ou palma são descartados. Com a caneta longe, os
  dedos funcionam normalmente.
- **Tela cheia imersiva** (`MainActivity.java`): barra de status, barra de navegação e a taskbar
  do One UI ficam escondidas. Um deslize a partir da borda as mostra por um instante.
- **Ainda não:** inclinação e rotação da caneta, o botão lateral da S Pen e o cursor do pincel
  enquanto a caneta paira. O winit 0.30 descarta esses dados no Android, então vão exigir ler os
  eventos na `MainActivity` e repassá-los ao Rust.

O que ainda falta no Android:
- Abrir e salvar arquivos pelo Storage Access Framework (o rfd não existe no Android). Hoje dá
  para criar documentos novos e pintar.
- Layout de toque: barra de ferramentas e painéis maiores, gesto de pinça para zoom.
- Orçamentos de memória e cache da GPU menores que os do desktop.
