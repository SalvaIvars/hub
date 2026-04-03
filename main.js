const { app, BrowserWindow, ipcMain, shell, dialog } = require('electron')
const path = require('path')
const fs = require('fs')
const Parser = require('rss-parser')

const parser = new Parser({ customFields: { item: ['media:group', 'media:thumbnail'] } })

// ── Data persistence ──────────────────────────────────────────────
const dataPath = path.join(app.getPath('userData'), 'sources.json')

function loadData() {
  if (!fs.existsSync(dataPath)) {
    const defaults = { sources: [], items: [], highlights: [], notes: [], links: [], settings: { refreshInterval: 0, accentColor: '', fontSize: 16 } }
    fs.writeFileSync(dataPath, JSON.stringify(defaults, null, 2))
    return defaults
  }
  const data = JSON.parse(fs.readFileSync(dataPath, 'utf-8'))
  if (!data.settings)    data.settings   = { refreshInterval: 0 }
  if (!data.highlights)  data.highlights  = []
  if (!data.notes)       data.notes       = []
  if (!data.links)       data.links       = []
  if (data.settings.fontSize    == null) data.settings.fontSize    = 16
  if (data.settings.accentColor == null) data.settings.accentColor = ''
  return data
}

const backupPath = path.join(app.getPath('userData'), 'sources.backup.json')

function saveData(data) {
  if (fs.existsSync(dataPath)) fs.copyFileSync(dataPath, backupPath)
  fs.writeFileSync(dataPath, JSON.stringify(data, null, 2))
}

// ── YouTube helpers ───────────────────────────────────────────────
function extractYouTubeInfo(url) {
  const tests = [
    [/youtube\.com\/channel\/(UC[a-zA-Z0-9_-]+)/, 'channel'],
    [/youtube\.com\/@([a-zA-Z0-9_.-]+)/,          'handle'],
    [/youtube\.com\/user\/([a-zA-Z0-9_.-]+)/,     'user'],
    [/youtube\.com\/watch\?.*v=([a-zA-Z0-9_-]{11})/, 'video'],
    [/youtu\.be\/([a-zA-Z0-9_-]{11})/,            'video'],
  ]
  for (const [re, type] of tests) {
    const m = url.match(re)
    if (m) return { type, id: m[1] }
  }
  return null
}

async function resolveYouTubeChannelId(info) {
  if (info.type === 'channel') return info.id
  const pageUrl =
    info.type === 'handle' ? `https://www.youtube.com/@${info.id}` :
    info.type === 'user'   ? `https://www.youtube.com/user/${info.id}` :
                              `https://www.youtube.com/watch?v=${info.id}`
  const res = await fetch(pageUrl, {
    headers: { 'User-Agent': 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36' }
  })
  if (!res.ok) throw new Error(`HTTP ${res.status} fetching YouTube page`)
  const html = await res.text()
  const m = html.match(/"channelId":"(UC[a-zA-Z0-9_-]+)"/) ||
            html.match(/"externalId":"(UC[a-zA-Z0-9_-]+)"/)
  if (!m) throw new Error('No se encontró el channel ID. Comprueba que la URL es válida.')
  return m[1]
}

// ── RSS fetching ──────────────────────────────────────────────────
const COMMON_FEED_PATHS = [
  '/feed', '/feed.xml', '/feed/', '/rss', '/rss.xml', '/rss/',
  '/atom.xml', '/atom/', '/index.xml', '/feeds/posts/default',
]

async function tryCommonFeedPaths(pageUrl) {
  const base = new URL(pageUrl).origin
  for (const p of COMMON_FEED_PATHS) {
    try {
      const feed = await parser.parseURL(base + p)
      if (feed.items && feed.items.length > 0) return feed
    } catch { /* try next */ }
  }
  throw new Error('No RSS feed found. Try pasting the feed URL directly.')
}

function discoverFeedUrl(html, baseUrl) {
  const re    = /<link[^>]+type="application\/(rss|atom)\+xml"[^>]*href="([^"]+)"/gi
  const reRev = /<link[^>]+href="([^"]+)"[^>]+type="application\/(rss|atom)\+xml"/gi
  const m = re.exec(html) || reRev.exec(html)
  if (!m) return null
  const href = m[2] || m[1]
  try { return new URL(href, baseUrl).href } catch { return href }
}

async function fetchFeed(source) {
  let feedUrl = source.feedUrl

  if (source.type === 'youtube') {
    if (!feedUrl || !feedUrl.includes('channel_id=')) {
      const info = extractYouTubeInfo(source.url)
      if (!info) throw new Error('URL de YouTube no reconocida.')
      const channelId = await resolveYouTubeChannelId(info)
      feedUrl = `https://www.youtube.com/feeds/videos.xml?channel_id=${channelId}`
      source.feedUrl = feedUrl
    }
  }

  if (!feedUrl) feedUrl = source.url
  if (!feedUrl) throw new Error('No URL configured')

  let feed
  try {
    feed = await parser.parseURL(feedUrl)
  } catch {
    const res = await fetch(feedUrl)
    if (!res.ok) throw new Error(`HTTP ${res.status} ${res.statusText}`)
    const text = await res.text()
    if (/<html[\s>]/i.test(text)) {
      const discovered = discoverFeedUrl(text, feedUrl)
      feed = discovered ? await parser.parseURL(discovered) : await tryCommonFeedPaths(feedUrl)
    } else {
      const clean = text.replace(/&(?!(?:#\d+|#x[\da-fA-F]+|[a-zA-Z]\w*);)/g, '&amp;')
      feed = await parser.parseString(clean)
    }
  }

  return feed.items.slice(0, 20).map(item => ({
    id: item.guid || item.link || item.title,
    sourceId: source.id,
    sourceName: source.name,
    sourceType: source.type,
    title: item.title || 'Untitled',
    link: item.link,
    date: item.pubDate || item.isoDate || null,
    thumbnail: extractThumbnail(item, source.type),
    read: false, saved: false, note: '', tags: [], readAt: null, later: false,
    savedAt: null, readMinutes: itemReadMinutes(item, source.type),
  }))
}

function itemReadMinutes(item, type) {
  if (type !== 'rss') return null
  const text = (item['content:encoded'] || item.content || item.contentSnippet || '')
    .replace(/<[^>]+>/g, '')
  const words = text.split(/\s+/).filter(Boolean).length
  return words > 50 ? Math.max(1, Math.round(words / 200)) : null
}

function extractThumbnail(item, type) {
  if (type === 'youtube') {
    const mg = item['media:group']
    if (mg && mg['media:thumbnail']?.[0]) return mg['media:thumbnail'][0].$.url
    if (item['media:thumbnail']?.$) return item['media:thumbnail'].$.url
    const m = (item.link || '').match(/v=([a-zA-Z0-9_-]{11})/)
    if (m) return `https://i.ytimg.com/vi/${m[1]}/mqdefault.jpg`
  }
  if (item.enclosure?.url) return item.enclosure.url
  const img = (item.content || item['content:encoded'] || '').match(/<img[^>]+src="([^"]+)"/i)
  return img ? img[1] : null
}

// ── Auto-refresh ──────────────────────────────────────────────────
let mainWindow = null
let refreshTimer = null

function setupAutoRefresh(intervalMinutes) {
  if (refreshTimer) { clearInterval(refreshTimer); refreshTimer = null }
  if (!intervalMinutes || intervalMinutes <= 0) return

  refreshTimer = setInterval(async () => {
    const data = loadData()
    if (!data.sources.length || !mainWindow) return
    const prevIds = new Set(data.items.map(i => i.id))
    const allItems = []
    for (const source of data.sources) {
      try {
        allItems.push(...await fetchFeed(source))
        source.lastFetchOk = true; source.lastFetchAt = new Date().toISOString()
      } catch {
        source.lastFetchOk = false; source.lastFetchAt = new Date().toISOString()
      }
    }
    const existingMap = new Map(data.items.map(i => [i.id, i]))
    const merged = allItems.map(item => ({
      ...item,
      read:    existingMap.get(item.id)?.read    ?? false,
      saved:   existingMap.get(item.id)?.saved   ?? false,
      note:    existingMap.get(item.id)?.note    ?? '',
      tags:    existingMap.get(item.id)?.tags    ?? [],
      readAt:  existingMap.get(item.id)?.readAt  ?? null,
      savedAt: existingMap.get(item.id)?.savedAt ?? null,
    }))
    const activeIds = new Set(data.sources.map(s => s.id))
    data.items = merged.filter(i => activeIds.has(i.sourceId))
    saveData(data)
    const newItems = data.items.filter(i => !prevIds.has(i.id))
    const priorityIds = new Set(data.sources.filter(s => s.priority).map(s => s.id))
    const priorityNames = [...new Set(newItems.filter(i => priorityIds.has(i.sourceId)).map(i => i.sourceName))]
    mainWindow.webContents.send('auto-refresh-done', { items: data.items, newCount: newItems.length, priorityNames })
  }, intervalMinutes * 60 * 1000)
}

// ── IPC handlers ──────────────────────────────────────────────────
ipcMain.handle('get-data', () => loadData())

ipcMain.handle('add-source', (_, source) => {
  const data = loadData()
  const newSource = {
    id: Date.now().toString(),
    name: source.name, url: source.url, feedUrl: source.feedUrl || '',
    type: source.type, category: source.category || 'General',
    priority: false, createdAt: new Date().toISOString()
  }
  data.sources.push(newSource)
  saveData(data)
  return newSource
})

ipcMain.handle('remove-source', (_, sourceId) => {
  const data = loadData()
  data.sources = data.sources.filter(s => s.id !== sourceId)
  data.items   = data.items.filter(i => i.sourceId !== sourceId)
  saveData(data)
  return true
})

ipcMain.handle('refresh-all', async () => {
  const data = loadData()
  const allItems = [], errors = []
  for (const source of data.sources) {
    try {
      allItems.push(...await fetchFeed(source))
      source.lastFetchOk = true; source.lastFetchAt = new Date().toISOString()
    } catch (err) {
      console.error(`Error fetching ${source.name}:`, err.message)
      errors.push({ name: source.name, error: err.message })
      source.lastFetchOk = false; source.lastFetchAt = new Date().toISOString()
    }
  }
  const existingMap = new Map(data.items.map(i => [i.id, i]))
  const merged = allItems.map(item => ({
    ...item,
    read:    existingMap.get(item.id)?.read    ?? false,
    saved:   existingMap.get(item.id)?.saved   ?? false,
    note:    existingMap.get(item.id)?.note    ?? '',
    tags:    existingMap.get(item.id)?.tags    ?? [],
    readAt:  existingMap.get(item.id)?.readAt  ?? null,
    savedAt: existingMap.get(item.id)?.savedAt ?? null,
  }))
  const activeIds = new Set(data.sources.map(s => s.id))
  data.items = merged.filter(i => activeIds.has(i.sourceId))
  saveData(data)
  return { items: data.items, errors }
})

ipcMain.handle('mark-read', (_, itemId) => {
  const data = loadData()
  const item = data.items.find(i => i.id === itemId)
  if (item) { item.read = true; item.readAt = item.readAt || new Date().toISOString() }
  saveData(data)
  return true
})

ipcMain.handle('toggle-saved', (_, itemId) => {
  const data = loadData()
  const item = data.items.find(i => i.id === itemId)
  if (item) {
    item.saved = !item.saved
    item.savedAt = item.saved ? new Date().toISOString() : null
  }
  saveData(data)
  return item
})

ipcMain.handle('mark-all-read', (_, sourceId) => {
  const data = loadData()
  const now = new Date().toISOString()
  data.items.filter(i => i.sourceId === sourceId).forEach(i => { i.read = true; i.readAt = i.readAt || now })
  saveData(data)
  return true
})

// ── Wikilink helpers ──────────────────────────────────────────────
function parseWikilinks(content) {
  const re = /\[\[([^\]]+)\]\]/g
  const matches = []
  let m
  while ((m = re.exec(content)) !== null) matches.push(m[1])
  return matches
}

function resolveWikilink(text, items) {
  const lower = text.toLowerCase()
  // Exact match first
  let match = items.find(i => i.title.toLowerCase() === lower)
  if (match) return match
  // Includes match
  match = items.find(i => i.title.toLowerCase().includes(lower))
  if (match) return match
  // Reverse includes
  match = items.find(i => lower.includes(i.title.toLowerCase()) && i.title.length > 3)
  return match || null
}

function updateLinksForNote(data, itemId, content) {
  // Remove old links from this item
  data.links = data.links.filter(l => l.fromItemId !== itemId)
  // Parse and resolve new links
  const wikiTexts = parseWikilinks(content)
  // Resolve against items + standalone note titles
  const candidates = data.items.filter(i => i.id !== itemId)
  const standaloneTargets = data.notes.filter(n => n.itemId.startsWith('standalone-') && n.itemId !== itemId).map(n => ({ id: n.itemId, title: n.title || '' }))
  const allTargets = [...candidates, ...standaloneTargets]
  const resolved = []
  for (const text of wikiTexts) {
    const target = resolveWikilink(text, allTargets)
    if (target && !resolved.find(r => r.toItemId === target.id)) {
      resolved.push({ fromItemId: itemId, toItemId: target.id, createdAt: new Date().toISOString() })
    }
  }
  data.links.push(...resolved)
  return resolved
}

ipcMain.handle('save-note', (_, { itemId, content, noteTags }) => {
  const data = loadData()
  const now = new Date().toISOString()
  // If itemId starts with 'standalone-', it's a standalone note (no item)
  const isStandalone = itemId && itemId.startsWith('standalone-')
  if (!isStandalone) {
    const item = data.items.find(i => i.id === itemId)
    if (item) item.note = content
  }
  // Upsert in notes[]
  const existing = data.notes.find(n => n.itemId === itemId)
  if (existing) {
    existing.content = content
    existing.updatedAt = now
    if (noteTags !== undefined) existing.noteTags = noteTags
  } else {
    data.notes.push({ id: Date.now().toString(), itemId, content, noteTags: noteTags || [], createdAt: now, updatedAt: now })
  }
  // Re-parse wikilinks
  const resolved = updateLinksForNote(data, itemId, content)
  saveData(data)
  return { ok: true, resolvedLinks: resolved }
})

ipcMain.handle('get-note', (_, itemId) => {
  const data = loadData()
  return data.notes.find(n => n.itemId === itemId) || null
})

ipcMain.handle('get-backlinks', (_, itemId) => {
  const data = loadData()
  const fromIds = data.links.filter(l => l.toItemId === itemId).map(l => l.fromItemId)
  // Include both items and standalone notes
  const items = data.items.filter(i => fromIds.includes(i.id))
  const standaloneNotes = data.notes.filter(n => fromIds.includes(n.itemId) && n.itemId.startsWith('standalone-'))
  return { items, standaloneNotes }
})

ipcMain.handle('get-all-links', () => {
  const data = loadData()
  return data.links
})

ipcMain.handle('get-all-notes', () => {
  const data = loadData()
  return data.notes
})

ipcMain.handle('search-notes', (_, query) => {
  const data = loadData()
  const q = query.toLowerCase()
  return data.notes.filter(n =>
    n.content.toLowerCase().includes(q) ||
    (n.noteTags || []).some(t => t.toLowerCase().includes(q))
  ).map(n => {
    const item = data.items.find(i => i.id === n.itemId)
    return { ...n, itemTitle: item?.title || n.itemId, itemSourceName: item?.sourceName || '' }
  })
})

ipcMain.handle('save-standalone-note', (_, { id, title, content, noteTags }) => {
  const data = loadData()
  const now = new Date().toISOString()
  const noteId = id || ('standalone-' + Date.now())
  const existing = data.notes.find(n => n.itemId === noteId)
  if (existing) {
    existing.content = content
    existing.updatedAt = now
    if (title !== undefined) existing.title = title
    if (noteTags !== undefined) existing.noteTags = noteTags
  } else {
    data.notes.push({ id: Date.now().toString(), itemId: noteId, title: title || 'Untitled', content, noteTags: noteTags || [], createdAt: now, updatedAt: now })
  }
  const resolved = updateLinksForNote(data, noteId, content)
  saveData(data)
  return { ok: true, noteId, resolvedLinks: resolved }
})

ipcMain.handle('delete-standalone-note', (_, noteId) => {
  const data = loadData()
  data.notes = data.notes.filter(n => n.itemId !== noteId)
  data.links = data.links.filter(l => l.fromItemId !== noteId && l.toItemId !== noteId)
  saveData(data)
  return true
})

ipcMain.handle('save-tags', (_, itemId, tags) => {
  const data = loadData()
  const item = data.items.find(i => i.id === itemId)
  if (item) item.tags = tags
  saveData(data)
  return true
})

ipcMain.handle('toggle-priority', (_, sourceId) => {
  const data = loadData()
  const src = data.sources.find(s => s.id === sourceId)
  if (src) src.priority = !src.priority
  saveData(data)
  return src
})

ipcMain.handle('get-settings', () => loadData().settings || { refreshInterval: 0 })

ipcMain.handle('save-settings', (_, settings) => {
  const data = loadData()
  data.settings = { ...data.settings, ...settings }
  saveData(data)
  if (settings.refreshInterval !== undefined) setupAutoRefresh(data.settings.refreshInterval)
  return true
})

ipcMain.handle('add-link', (_, { title, url }) => {
  const data = loadData()
  const item = {
    id: 'link-' + Date.now(), sourceId: 'manual', sourceName: 'Links',
    sourceType: 'manual', title, link: url,
    date: new Date().toISOString(), thumbnail: null,
    read: false, saved: true, note: '', tags: [], readAt: null,
    savedAt: new Date().toISOString(),
  }
  data.items.unshift(item)
  saveData(data)
  return item
})

ipcMain.handle('get-stats', () => {
  const data = loadData()
  const now = Date.now(), dayMs = 864e5
  const todayStr = new Date().toDateString()
  const readItems = data.items.filter(i => i.readAt)
  const readThisWeek = readItems.filter(i => now - new Date(i.readAt) < 7 * dayMs)
  const readToday    = readItems.filter(i => new Date(i.readAt).toDateString() === todayStr)
  const counts = {}
  readThisWeek.forEach(i => { counts[i.sourceName] = (counts[i.sourceName] || 0) + 1 })
  const topSources = Object.entries(counts).sort((a, b) => b[1] - a[1]).slice(0, 5).map(([name, count]) => ({ name, count }))
  const days = new Set(readItems.map(i => new Date(i.readAt).toDateString()))
  let streak = 0
  const d = new Date()
  while (days.has(d.toDateString())) { streak++; d.setDate(d.getDate() - 1) }
  return { readToday: readToday.length, readThisWeek: readThisWeek.length, totalRead: readItems.length, totalSaved: data.items.filter(i => i.saved).length, topSources, streak }
})

ipcMain.handle('export-markdown', async () => {
  const data = loadData()
  const saved = data.items.filter(i => i.saved)
  if (!saved.length) return { error: 'No hay items guardados.' }
  const md = saved.map(i => {
    const parts = [`## [${i.title}](${i.link})`, `**${i.sourceName}** · ${i.date ? new Date(i.date).toLocaleDateString() : ''}`]
    if (i.tags?.length) parts.push(i.tags.map(t => `#${t}`).join(' '))
    if (i.note) parts.push(`\n> ${i.note.replace(/\n/g, '\n> ')}`)
    return parts.join('\n')
  }).join('\n\n---\n\n')
  const { filePath } = await dialog.showSaveDialog(mainWindow, { defaultPath: 'hub-saved.md', filters: [{ name: 'Markdown', extensions: ['md'] }] })
  if (!filePath) return { cancelled: true }
  fs.writeFileSync(filePath, md, 'utf-8')
  return { ok: true }
})

ipcMain.handle('export-opml', async () => {
  const data = loadData()
  const esc = s => (s || '').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;')
  const lines = data.sources.filter(s => s.type !== 'manual').map(s =>
    `    <outline type="rss" text="${esc(s.name)}" title="${esc(s.name)}" xmlUrl="${esc(s.feedUrl || s.url)}" htmlUrl="${esc(s.url)}"/>`
  )
  const opml = `<?xml version="1.0" encoding="UTF-8"?>\n<opml version="2.0">\n  <head><title>Hub feeds</title></head>\n  <body>\n${lines.join('\n')}\n  </body>\n</opml>`
  const { filePath } = await dialog.showSaveDialog(mainWindow, { defaultPath: 'hub-feeds.opml', filters: [{ name: 'OPML', extensions: ['opml', 'xml'] }] })
  if (!filePath) return { cancelled: true }
  fs.writeFileSync(filePath, opml, 'utf-8')
  return { ok: true }
})

ipcMain.handle('import-opml', async () => {
  const { filePaths } = await dialog.showOpenDialog(mainWindow, { filters: [{ name: 'OPML', extensions: ['opml', 'xml'] }], properties: ['openFile'] })
  if (!filePaths?.length) return { cancelled: true }
  const xml = fs.readFileSync(filePaths[0], 'utf-8')
  const outlineRe = /<outline[^>]+>/gi
  const imported = []
  let m
  while ((m = outlineRe.exec(xml)) !== null) {
    const tag = m[0]
    const xmlUrl = tag.match(/xmlUrl="([^"]+)"/)
    const textM  = tag.match(/text="([^"]+)"/) || tag.match(/title="([^"]+)"/)
    if (!xmlUrl || !textM) continue
    const htmlUrl = tag.match(/htmlUrl="([^"]+)"/)
    imported.push({ name: textM[1], feedUrl: xmlUrl[1], url: htmlUrl ? htmlUrl[1] : xmlUrl[1], type: 'rss' })
  }
  if (!imported.length) return { error: 'No feeds found in OPML file.' }
  const data = loadData()
  const existing = new Set(data.sources.map(s => s.feedUrl || s.url))
  let added = 0
  for (const src of imported) {
    if (existing.has(src.feedUrl)) continue
    data.sources.push({ id: Date.now().toString() + Math.random(), ...src, category: 'General', priority: false, createdAt: new Date().toISOString() })
    added++
  }
  saveData(data)
  return { added, total: imported.length }
})

ipcMain.handle('open-external', (_, url) => shell.openExternal(url))

ipcMain.handle('reorder-sources', (_, orderedIds) => {
  const data = loadData()
  const map = new Map(data.sources.map(s => [s.id, s]))
  const reordered = orderedIds.map(id => map.get(id)).filter(Boolean)
  const missing = data.sources.filter(s => !orderedIds.includes(s.id))
  data.sources = [...reordered, ...missing]
  saveData(data)
  return true
})

ipcMain.handle('toggle-mute', (_, sourceId) => {
  const data = loadData()
  const src = data.sources.find(s => s.id === sourceId)
  if (src) src.muted = !src.muted
  saveData(data)
  return src
})

ipcMain.handle('update-source', (_, id, updates) => {
  const data = loadData()
  const src = data.sources.find(s => s.id === id)
  if (src) {
    if (updates.name) { src.name = updates.name; data.items.filter(i => i.sourceId === id).forEach(i => { i.sourceName = updates.name }) }
    if (updates.category !== undefined) src.category = updates.category
  }
  saveData(data)
  return src
})

ipcMain.handle('validate-feed', async (_, url) => {
  try {
    const feed = await parser.parseURL(url)
    return { ok: true, itemCount: feed.items?.length ?? 0 }
  } catch {
    try {
      const res = await fetch(url)
      if (!res.ok) return { ok: false, error: `HTTP ${res.status}` }
      const text = await res.text()
      if (/<html[\s>]/i.test(text)) {
        const discovered = discoverFeedUrl(text, url)
        if (discovered) { await parser.parseURL(discovered); return { ok: true } }
        return { ok: false, error: 'No RSS feed found at this URL' }
      }
      const clean = text.replace(/&(?!(?:#\d+|#x[\da-fA-F]+|[a-zA-Z]\w*);)/g, '&amp;')
      const feed2 = await parser.parseString(clean)
      return { ok: true, itemCount: feed2.items?.length ?? 0 }
    } catch (err) {
      return { ok: false, error: err.message }
    }
  }
})

ipcMain.handle('get-source-stats', () => {
  const data = loadData()
  const stats = {}
  for (const src of data.sources) {
    const items = data.items.filter(i => i.sourceId === src.id)
    stats[src.id] = { total: items.length, unread: items.filter(i => !i.read).length }
  }
  return stats
})

ipcMain.handle('toggle-later', (_, itemId) => {
  const data = loadData()
  const item = data.items.find(i => i.id === itemId)
  if (item) item.later = !item.later
  saveData(data)
  return item
})

// ── Readable extraction ───────────────────────────────────────────
function extractReadable(html) {
  const stripTags = s => s
    .replace(/<[^>]+>/g, ' ')
    .replace(/&amp;/g, '&').replace(/&lt;/g, '<').replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&nbsp;/g, ' ')
    .replace(/&#\d+;/g, ' ').replace(/\s+/g, ' ').trim()

  const clean = html
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<style[\s\S]*?<\/style>/gi, '')
    .replace(/<iframe[\s\S]*?<\/iframe>/gi, '')
    .replace(/<!--[\s\S]*?-->/g, '')

  const content =
    clean.match(/<article[^>]*>([\s\S]*?)<\/article>/i)?.[1] ||
    clean.match(/<main[^>]*>([\s\S]*?)<\/main>/i)?.[1] ||
    clean.match(/<body[^>]*>([\s\S]*?)<\/body>/i)?.[1] ||
    clean

  const blocks = []
  const re = /<(h[1-3]|p|blockquote)[^>]*>([\s\S]*?)<\/\1>/gi
  let m
  while ((m = re.exec(content)) !== null) {
    const type = m[1].toLowerCase()
    const text = stripTags(m[2])
    if (text.length > (type === 'p' ? 25 : 1)) blocks.push({ type, text })
  }

  // Fallback: split plain text into paragraphs
  if (blocks.length < 3) {
    stripTags(content).split(/\n{2,}/).forEach(para => {
      if (para.trim().length > 30) blocks.push({ type: 'p', text: para.trim() })
    })
  }

  return blocks
}

ipcMain.handle('fetch-readable', async (_, url) => {
  try {
    const res = await fetch(url, {
      headers: { 'User-Agent': 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36' }
    })
    if (!res.ok) return { error: `HTTP ${res.status}` }
    const blocks = extractReadable(await res.text())
    return { ok: true, blocks }
  } catch (err) {
    return { error: err.message }
  }
})

ipcMain.handle('save-highlight', (_, { itemId, text }) => {
  const data = loadData()
  if (!data.highlights) data.highlights = []
  if (!data.highlights.find(h => h.itemId === itemId && h.text === text)) {
    data.highlights.push({ id: Date.now().toString(), itemId, text, createdAt: new Date().toISOString() })
    saveData(data)
  }
  return data.highlights
})

ipcMain.handle('get-highlights', () => {
  const data = loadData()
  return data.highlights || []
})

// ── Window ────────────────────────────────────────────────────────
function createWindow() {
  mainWindow = new BrowserWindow({
    show: false,
    width: 1280, height: 800, minWidth: 800, minHeight: 500,
    titleBarStyle: process.platform === 'darwin' ? 'hiddenInset' : 'default',
    backgroundColor: '#0f0f0f',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      webviewTag: true,
    }
  })
  mainWindow.once('ready-to-show', () => mainWindow.show())
  mainWindow.loadFile(path.join(__dirname, 'index.html'))
  const data = loadData()
  if (data.settings?.refreshInterval) setupAutoRefresh(data.settings.refreshInterval)
}

app.whenReady().then(createWindow)
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit() })
app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow() })
