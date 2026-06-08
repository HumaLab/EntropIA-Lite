import { eq, asc } from 'drizzle-orm'
import type { DrizzleClient, DbClient } from '../types'
import { assets } from '../schema'

export type Asset = typeof assets.$inferSelect
export type NewAsset = typeof assets.$inferInsert

type AssetRow = {
  id: string
  item_id: string
  path: string
  type: string
  sort_index: number
  size: number | null
  created_at: number
}

function orderAssetsForDisplay(rows: Asset[]): Asset[] {
  const preservesPageOrder = rows.length > 1 && rows.some((asset) => asset.sortIndex !== 0)
  return [...rows].sort((a, b) => {
    if (preservesPageOrder) {
      const bySortIndex = a.sortIndex - b.sortIndex
      if (bySortIndex !== 0) return bySortIndex
    }

    const byPath = a.path.localeCompare(b.path, undefined, { sensitivity: 'base' })
    if (byPath !== 0) return byPath
    return a.id.localeCompare(b.id)
  })
}

export class AssetRepo {
  constructor(
    private db: DrizzleClient,
    private rawClient?: DbClient
  ) {}

  async create(data: Omit<NewAsset, 'id' | 'createdAt'>): Promise<Asset> {
    const createdAsset: Asset = {
      id: crypto.randomUUID(),
      itemId: data.itemId,
      path: data.path,
      type: data.type,
      sortIndex: data.sortIndex ?? 0,
      size: data.size ?? null,
      createdAt: Date.now(),
    }

    if (this.rawClient) {
      // Validate that the parent item exists before inserting (FK constraint)
      const itemExists = await this.rawClient.select('SELECT id FROM items WHERE id = ?', [
        createdAsset.itemId,
      ])
      if (itemExists.length === 0) {
        throw new Error(`Cannot create asset: item "${createdAsset.itemId}" does not exist`)
      }

      await this.rawClient.execute(
        'INSERT INTO assets (id, item_id, path, type, sort_index, size, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)',
        [
          createdAsset.id,
          createdAsset.itemId,
          createdAsset.path,
          createdAsset.type,
          createdAsset.sortIndex,
          createdAsset.size,
          createdAsset.createdAt,
        ]
      )
    } else {
      await this.db.insert(assets).values(createdAsset)
    }

    return createdAsset
  }

  async findByItem(itemId: string): Promise<Asset[]> {
    if (this.rawClient) {
      const rows = await this.rawClient.select<AssetRow>(
        `SELECT id, item_id, path, type, sort_index, size, created_at
         FROM assets
         WHERE item_id = ?
         ORDER BY path COLLATE NOCASE ASC, id ASC`,
        [itemId]
      )

      return orderAssetsForDisplay(rows.map((row) => ({
        id: row.id,
        itemId: row.item_id,
        path: row.path,
        type: row.type,
        sortIndex: row.sort_index,
        size: row.size,
        createdAt: row.created_at,
      })))
    }

    const rows = await this.db
      .select()
      .from(assets)
      .where(eq(assets.itemId, itemId))
      .orderBy(asc(assets.path), asc(assets.id))
    return orderAssetsForDisplay(rows)
  }

  async findById(id: string): Promise<Asset | null> {
    const rows = await this.db.select().from(assets).where(eq(assets.id, id))

    return rows[0] ?? null
  }

  async delete(id: string): Promise<void> {
    await this.db.delete(assets).where(eq(assets.id, id))
  }

  /**
   * Update the path of an asset (e.g. after JPEG → PNG conversion).
   */
  async updatePath(id: string, newPath: string): Promise<void> {
    if (this.rawClient) {
      await this.rawClient.execute('UPDATE assets SET path = ? WHERE id = ?', [
        newPath,
        id,
      ])
    } else {
      await this.db.update(assets).set({ path: newPath }).where(eq(assets.id, id))
    }
  }

  /**
   * Delete an asset and all its dependent records
   * in a single atomic transaction. Returns the deleted asset record
   * so the caller can remove the associated file from the filesystem.
   *
   * @throws Error if the asset is not found
   * @throws Error if the transaction fails
   */
  async deleteWithCascade(id: string): Promise<Asset> {
    if (!this.rawClient) {
      throw new Error('deleteWithCascade requires a rawClient for transactional execution')
    }

    // Step 1: Fetch the asset to get its path and verify it exists
    const asset = await this.findById(id)
    if (!asset) {
      throw new Error(`Asset not found: ${id}`)
    }

    // Step 2: Execute all deletes in a single explicit transaction.
    const esc = id.replace(/'/g, "''")
    try {
      await this.rawClient.executeBatch(`
        BEGIN;
        DELETE FROM extractions WHERE asset_id = '${esc}';
        DELETE FROM layouts WHERE asset_id = '${esc}';
        DELETE FROM transcriptions WHERE asset_id = '${esc}';
        DELETE FROM llm_results WHERE target_id = '${esc}' AND (target_type = 'asset' OR target_type = 'unknown');
        DELETE FROM annotations WHERE asset_id = '${esc}';
        DELETE FROM entities WHERE asset_id = '${esc}';
        DELETE FROM triples WHERE asset_id = '${esc}';
        DELETE FROM vec_assets WHERE asset_id = '${esc}';
        DELETE FROM assets WHERE id = '${esc}';
        COMMIT;
      `)
    } catch (e) {
      // Transaction failed — rethrow with context
      throw new Error(
        `Failed to delete asset cascade for ${id}: ${e instanceof Error ? e.message : String(e)}`
      )
    }

    return asset
  }
}
