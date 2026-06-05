<script lang="ts">
  import {
    ChevronDown,
    ChevronLeft,
    ChevronRight,
    ChevronUp,
    CirclePlay,
    Copy,
    Crop,
    Database,
    Download,
    Eraser,
    Expand,
    Eye,
    EyeOff,
    ExternalLink,
    File,
    FileAudio,
    FileImage,
    FileText,
    Folder,
    FolderPlus,
    Hand,
    Moon,
    PanelLeft,
    PanelLeftClose,
    Pause,
    Pencil,
    Play,
    RotateCcw,
    RotateCw,
    Search,
    SearchX,
    Settings,
    SkipBack,
    SkipForward,
    Square,
    Trash2,
    Underline,
    Undo2,
    Volume2,
    X,
    ZoomIn,
    ZoomOut,
  } from '@lucide/svelte'
  import type { ActionIconName } from './ActionIcon.types'

  let { name, size = 16 }: { name: ActionIconName; size?: number } = $props()
</script>

{#if name === 'chevron-left'}
  <ChevronLeft {size} aria-hidden="true" />
{:else if name === 'chevron-right'}
  <ChevronRight {size} aria-hidden="true" />
{:else if name === 'chevron-up'}
  <ChevronUp {size} aria-hidden="true" />
{:else if name === 'chevron-down'}
  <ChevronDown {size} aria-hidden="true" />
{:else if name === 'close'}
  <X {size} aria-hidden="true" />
{:else if name === 'circle-play'}
  <CirclePlay {size} aria-hidden="true" />
{:else if name === 'copy'}
  <Copy {size} aria-hidden="true" />
{:else if name === 'crop'}
  <Crop {size} aria-hidden="true" />
{:else if name === 'database'}
  <Database {size} aria-hidden="true" />
{:else if name === 'delete'}
  <Trash2 {size} aria-hidden="true" />
{:else if name === 'download'}
  <Download {size} aria-hidden="true" />
{:else if name === 'edit'}
  <Pencil {size} aria-hidden="true" />
{:else if name === 'eraser'}
  <Eraser {size} aria-hidden="true" />
{:else if name === 'expand'}
  <Expand {size} aria-hidden="true" />
{:else if name === 'eye'}
  <Eye {size} aria-hidden="true" />
{:else if name === 'eye-off'}
  <EyeOff {size} aria-hidden="true" />
{:else if name === 'external-link'}
  <ExternalLink {size} aria-hidden="true" />
{:else if name === 'file'}
  <File {size} aria-hidden="true" />
{:else if name === 'file-audio'}
  <FileAudio {size} aria-hidden="true" />
{:else if name === 'file-image'}
  <FileImage {size} aria-hidden="true" />
{:else if name === 'file-text'}
  <FileText {size} aria-hidden="true" />
{:else if name === 'folder'}
  <Folder {size} aria-hidden="true" />
{:else if name === 'folder-plus'}
  <FolderPlus {size} aria-hidden="true" />
{:else if name === 'hand'}
  <Hand {size} aria-hidden="true" />
{:else if name === 'moon'}
  <Moon {size} aria-hidden="true" />
{:else if name === 'panel-left'}
  <PanelLeft {size} aria-hidden="true" />
{:else if name === 'panel-left-close'}
  <PanelLeftClose {size} aria-hidden="true" />
{:else if name === 'pause'}
  <Pause {size} aria-hidden="true" />
{:else if name === 'pencil'}
  <Pencil {size} aria-hidden="true" />
{:else if name === 'play'}
  <Play {size} aria-hidden="true" />
{:else if name === 'rectangle'}
  <Square {size} aria-hidden="true" />
{:else if name === 'rotate-ccw'}
  <RotateCcw {size} aria-hidden="true" />
{:else if name === 'rotate-cw'}
  <RotateCw {size} aria-hidden="true" />
{:else if name === 'search'}
  <Search {size} aria-hidden="true" />
{:else if name === 'search-x'}
  <SearchX {size} aria-hidden="true" />
{:else if name === 'settings'}
  <Settings {size} aria-hidden="true" />
{:else if name === 'skip-back'}
  <SkipBack {size} aria-hidden="true" />
{:else if name === 'skip-forward'}
  <SkipForward {size} aria-hidden="true" />
{:else if name === 'underline'}
  <Underline {size} aria-hidden="true" />
{:else if name === 'undo'}
  <Undo2 {size} aria-hidden="true" />
{:else if name === 'volume'}
  <Volume2 {size} aria-hidden="true" />
{:else if name === 'zoom-in'}
  <ZoomIn {size} aria-hidden="true" />
{:else if name === 'zoom-out'}
  <ZoomOut {size} aria-hidden="true" />
{/if}
