import { describe, expect, it } from 'vitest'
import {
  IMPORTED_FILE_METADATA_KEY,
  buildTechnicalMetadata,
  getAssetPathLabel,
  getAssetTypeLabel,
  mergeReservedMetadata,
  normalizeMetadataKey,
  parseImportedFileMetadata,
  parseMetadataRecord,
} from './item-metadata'
import type { Asset, Collection, Item } from '@entropia/store'

describe('item metadata helpers', () => {
  const item = {
    id: 'item-1',
    title: 'Documento de prueba',
  } as Item

  const asset = {
    id: 'asset-1',
    type: 'pdf',
    path: 'uploads/archivo.pdf',
    size: 1536,
  } as Asset

  const collection = {
    id: 'collection-1',
    name: 'Archivo histórico',
  } as Collection

  it('parses custom metadata without exposing reserved imported-file metadata', () => {
    const metadata = parseMetadataRecord(
      JSON.stringify({
        autor: 'Mariano Moreno',
        pages: 3,
        [IMPORTED_FILE_METADATA_KEY]: { originalPath: 'C:/privado/archivo.pdf' },
      })
    )

    expect(metadata).toEqual({ autor: 'Mariano Moreno', pages: '3' })
  })

  it('preserves reserved imported-file metadata when custom metadata is saved', () => {
    const source = JSON.stringify({
      [IMPORTED_FILE_METADATA_KEY]: {
        originalName: 'fuente.pdf',
        originalPath: 'C:/fuente/fuente.pdf',
      },
    })

    expect(mergeReservedMetadata({ autor: 'Belgrano' }, source)).toEqual({
      autor: 'Belgrano',
      [IMPORTED_FILE_METADATA_KEY]: {
        originalName: 'fuente.pdf',
        originalPath: 'C:/fuente/fuente.pdf',
      },
    })
  })

  it('extracts imported file metadata from the reserved key', () => {
    expect(
      parseImportedFileMetadata(
        JSON.stringify({ [IMPORTED_FILE_METADATA_KEY]: { originalName: 'acta.pdf' } })
      )
    ).toEqual({ originalName: 'acta.pdf' })
  })

  it('builds technical metadata and avoids duplicating custom metadata aliases', () => {
    const metadata = buildTechnicalMetadata({
      item,
      selectedAsset: asset,
      collection,
      originalFileMetadata: {
        originalName: 'acta-original.pdf',
        sizeBytes: 2048,
        readonly: true,
      },
      customMetadataKeys: new Set(['ruta interna'].map(normalizeMetadataKey)),
    })

    expect(metadata).toEqual(
      expect.arrayContaining([
        { label: 'Nombre del archivo', value: 'archivo.pdf' },
        { label: 'Tipo de archivo', value: 'PDF' },
        { label: 'Extensión', value: '.pdf' },
        { label: 'Tamaño', value: '1.5 KB' },
        { label: 'Documento ID', value: 'item-1' },
        { label: 'Asset ID', value: 'asset-1' },
        { label: 'Colección', value: 'Archivo histórico' },
        { label: 'Nombre original', value: 'acta-original.pdf' },
        { label: 'Tamaño original', value: '2.0 KB' },
        { label: 'Solo lectura', value: 'Sí' },
      ])
    )
    expect(metadata.some((entry) => entry.label === 'Ruta interna')).toBe(false)
  })

  it('formats asset labels consistently', () => {
    expect(getAssetPathLabel('C:\\documentos\\imagen.png')).toBe('imagen.png')
    expect(getAssetTypeLabel('image')).toBe('IMAGE')
    expect(getAssetTypeLabel('')).toBe('ASSET')
  })
})
