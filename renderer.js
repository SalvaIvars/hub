// ── State ─────────────────────────────────────────────────────────
let state        = { sources: [], items: [] }
let activeFilter   = 'all'
let activeCategory = 'all'
let activeTag      = ''
let searchQuery    = ''
let sortOrder      = 'newest'
let ageFilter      = 0          // days, 0 = all
let viewMode       = localStorage.getItem('hub-view') || 'expanded'
let hideRead       = localStorage.getItem('hub-hide-read') === 'true'
let visibleCount   = 20
let feedObserver   = null
let readerItem     = null
let scrollMemory   = new Map()  // filterId+category → scrollTop
let pendingDelete  = null       // { source, timer }
let dragSrcId      = null
let highlights     = []
let readerMode     = false
let noteMode       = false
let graphInstance  = null
let pendingHighlightText = ''   // selected text awaiting highlight save
let notesFilter = 'all'
let notesSearchQuery = ''
let activeStandaloneId = null
let graphFilterCat = 'all'
let graphFilterTag = 'all'
let readerHistory  = []        // breadcrumb stack for wikilink navigation
let zenMode        = false

// ── Init ──────────────────────────────────────────────────────────
async function init() {
  state = await window.hub.getData()
  const settings = await window.hub.getSettings()
  highlights = await window.hub.getHighlights()

  document.getElementById('refresh-interval').value = (settings.refreshInterval || 0).toString()

  // Apply persisted visual settings
  applyAccentColor(settings.accentColor || '')
  applyFontSize(settings.fontSize  || 16)

  applyViewMode()
  applyHideReadBtn()
  renderSidebar()
  renderCategoryPills()
  renderFeed()
  updateStatusBar()
  checkDigest()

  // Fade out splash
  const splash = document.getElementById('splash')
  splash.classList.add('fading')
  setTimeout(() => splash.remove(), 400)

  window.hub.onAutoRefresh(({ items, newCount, priorityNames }) => {
    state.items = items
    renderSidebar()
    renderCategoryPills()
    applyFilters()
    updateStatusBar(); setLastRefresh()
    if (newCount > 0 && priorityNames.length > 0) {
      new Notification('Hub', {
        body: `${newCount} new item${newCount > 1 ? 's' : ''} — ${priorityNames.join(', ')}`
      })
    }
  })
}

// ── Filtering ─────────────────────────────────────────────────────
function getFilteredItems() {
  const pendingIds = new Set(state.sources.filter(s => s._pendingDelete).map(s => s.id))
  let items = state.items.filter(i => !pendingIds.has(i.sourceId))

  // Exclude muted sources when showing all (not directly selected)
  if (activeFilter === 'all') {
    const mutedIds = new Set(state.sources.filter(s => s.muted && !s._pendingDelete).map(s => s.id))
    if (mutedIds.size) items = items.filter(i => !mutedIds.has(i.sourceId))
  }

  if (activeFilter !== 'all') {
    items = items.filter(i => i.sourceId === activeFilter)
  }

  if (activeCategory === 'saved') {
    items = items.filter(i => i.saved)
  } else if (activeCategory === 'later') {
    items = items.filter(i => i.later)
  } else if (activeCategory === 'youtube' || activeCategory === 'rss' || activeCategory === 'manual') {
    items = items.filter(i => i.sourceType === activeCategory)
  } else if (activeCategory !== 'all') {
    items = items.filter(i => {
      const src = state.sources.find(s => s.id === i.sourceId)
      return src && src.category === activeCategory
    })
  }

  if (activeTag) {
    items = items.filter(i => i.tags && i.tags.includes(activeTag))
  }

  if (ageFilter > 0) {
    const cutoff = Date.now() - ageFilter * 864e5
    items = items.filter(i => i.date && new Date(i.date) > cutoff)
  }

  if (hideRead) {
    items = items.filter(i => !i.read)
  }

  if (searchQuery) {
    const q = searchQuery.toLowerCase()
    items = items.filter(i =>
      i.title.toLowerCase().includes(q) ||
      i.sourceName.toLowerCase().includes(q)
    )
  }

  // Deduplicate by link (keep first occurrence)
  const seenLinks = new Set()
  items = items.filter(i => {
    if (!i.link) return true
    if (seenLinks.has(i.link)) return false
    seenLinks.add(i.link); return true
  })

  return items.slice().sort((a, b) => {
    if (sortOrder === 'oldest') return (a.date ? new Date(a.date) : 0) - (b.date ? new Date(b.date) : 0)
    if (sortOrder === 'source') return a.sourceName.localeCompare(b.sourceName)
    return (b.date ? new Date(b.date) : 0) - (a.date ? new Date(a.date) : 0)
  })
}

function applyFilters() {
  const key = activeFilter + '|' + activeCategory
  const feed = document.getElementById('feed')
  scrollMemory.set(key, feed.scrollTop)
  visibleCount = 20
  renderFeed()
}

// ── Sidebar ───────────────────────────────────────────────────────
function renderSidebar() {
  const list = document.getElementById('source-list')
  list.innerHTML = ''
  list.appendChild(navItem('All', 'all', null))

  const categories = {}
  for (const s of state.sources.filter(s => !s._pendingDelete)) {
    const cat = s.category || 'General'
    if (!categories[cat]) categories[cat] = []
    categories[cat].push(s)
  }

  for (const [cat, sources] of Object.entries(categories)) {
    const sec = document.createElement('div')
    sec.className = 'nav-section'
    sec.textContent = cat
    list.appendChild(sec)
    for (const s of sources) list.appendChild(navItem(s.name, s.id, s))
  }

  // Manual links virtual entry
  if (state.items.some(i => i.sourceType === 'manual')) {
    const sec = document.createElement('div')
    sec.className = 'nav-section'
    sec.textContent = 'Links'
    list.appendChild(sec)
    list.appendChild(navItem('Saved links', 'manual-links', null, 'link'))
  }
}

function navItem(name, filter, source, iconType) {
  const el = document.createElement('div')
  el.className = 'nav-item' +
    (activeFilter === filter ? ' active' : '') +
    (source?.muted ? ' muted' : '')
  el.dataset.filter = filter

  if (source) {
    // Status dot
    const dot = document.createElement('span')
    dot.className = 'source-status' +
      (source.lastFetchAt == null ? '' : source.lastFetchOk ? ' ok' : ' error')
    dot.title = source.lastFetchAt
      ? (source.lastFetchOk ? 'Last fetch OK' : 'Last fetch failed')
      : 'Never fetched'
    el.appendChild(dot)

    // Drag & drop
    el.draggable = true
    el.dataset.sourceId = source.id
    el.addEventListener('dragstart', e => {
      dragSrcId = source.id
      el.classList.add('dragging')
      e.dataTransfer.effectAllowed = 'move'
    })
    el.addEventListener('dragend', () => el.classList.remove('dragging'))
    el.addEventListener('dragover', e => {
      e.preventDefault(); e.dataTransfer.dropEffect = 'move'
      document.querySelectorAll('.nav-item.drag-over').forEach(n => n.classList.remove('drag-over'))
      el.classList.add('drag-over')
    })
    el.addEventListener('dragleave', () => el.classList.remove('drag-over'))
    el.addEventListener('drop', async e => {
      e.preventDefault()
      el.classList.remove('drag-over')
      if (!dragSrcId || dragSrcId === source.id) return
      const fromIdx = state.sources.findIndex(s => s.id === dragSrcId)
      const toIdx   = state.sources.findIndex(s => s.id === source.id)
      if (fromIdx < 0 || toIdx < 0) return
      const moved = state.sources.splice(fromIdx, 1)[0]
      state.sources.splice(toIdx, 0, moved)
      dragSrcId = null
      await window.hub.reorderSources(state.sources.map(s => s.id))
      renderSidebar()
    })
  }

  const icon = document.createElement('span')
  icon.className = 'source-icon'
  icon.textContent = iconType === 'link' ? '⊞' : source ? (source.type === 'youtube' ? '▶' : '◉') : '◈'

  const nameEl = document.createElement('span')
  nameEl.className = 'source-name'
  nameEl.textContent = name
  el.appendChild(icon)
  el.appendChild(nameEl)

  if (source) {
    const srcItems = state.items.filter(i => i.sourceId === source.id)
    const unread   = srcItems.filter(i => !i.read).length
    const total    = srcItems.length

    // Total badge
    if (total > 0) {
      const totalBadge = document.createElement('span')
      totalBadge.className = 'badge-total'
      totalBadge.textContent = total
      totalBadge.title = `${total} posts total`
      el.appendChild(totalBadge)
    }

    // Unread badge
    if (unread > 0) {
      const badge = document.createElement('span')
      badge.className = 'badge'
      badge.textContent = unread
      el.appendChild(badge)
    }

    // Priority star
    const star = document.createElement('button')
    star.className = 'priority-btn' + (source.priority ? ' on' : '')
    star.textContent = '★'
    star.title = source.priority ? 'Remove priority' : 'Mark as priority'
    star.addEventListener('click', async e => {
      e.stopPropagation()
      const updated = await window.hub.togglePriority(source.id)
      if (updated) {
        source.priority = updated.priority
        star.className = 'priority-btn' + (source.priority ? ' on' : '')
        star.title = source.priority ? 'Remove priority' : 'Mark as priority'
      }
    })
    el.appendChild(star)

    // Mute button
    const muteBtn = document.createElement('button')
    muteBtn.className = 'mute-btn'
    muteBtn.textContent = source.muted ? '🔇' : '🔕'
    muteBtn.title = source.muted ? 'Unmute source' : 'Mute source (hide from All)'
    muteBtn.addEventListener('click', async e => {
      e.stopPropagation()
      const updated = await window.hub.toggleMute(source.id)
      if (updated) {
        source.muted = updated.muted
        el.classList.toggle('muted', source.muted)
        muteBtn.textContent = source.muted ? '🔇' : '🔕'
        muteBtn.title = source.muted ? 'Unmute source' : 'Mute source (hide from All)'
        applyFilters()
      }
    })
    el.appendChild(muteBtn)

    // Edit button
    const editBtn = document.createElement('button')
    editBtn.className = 'edit-btn'
    editBtn.textContent = '✎'
    editBtn.title = 'Edit source'
    editBtn.addEventListener('click', e => {
      e.stopPropagation()
      el.classList.add('editing')
      nameEl.style.display = 'none'
      editBtn.style.display = 'none'
      muteBtn.style.display = 'none'
      star.style.display = 'none'

      const wrap = document.createElement('div')
      wrap.className = 'edit-wrap'

      const nameInput = document.createElement('input')
      nameInput.className = 'edit-name'
      nameInput.value = source.name
      nameInput.placeholder = 'Name'

      const catInput = document.createElement('input')
      catInput.className = 'edit-cat'
      catInput.value = source.category || 'General'
      catInput.placeholder = 'Category'

      const saveBtn = document.createElement('button')
      saveBtn.className = 'save-edit-btn'
      saveBtn.textContent = 'Save'

      const cancelEdit = () => {
        wrap.remove()
        el.classList.remove('editing')
        nameEl.style.display = ''
        editBtn.style.display = ''
        muteBtn.style.display = ''
        star.style.display = ''
      }

      saveBtn.addEventListener('click', async ev => {
        ev.stopPropagation()
        const newName = nameInput.value.trim() || source.name
        const newCat  = catInput.value.trim()  || 'General'
        const updated = await window.hub.updateSource(source.id, { name: newName, category: newCat })
        if (updated) {
          source.name = newName; source.category = newCat
          state.items.filter(i => i.sourceId === source.id).forEach(i => { i.sourceName = newName })
        }
        cancelEdit()
        renderSidebar(); renderCategoryPills(); renderFeed()
      })

      nameInput.addEventListener('keydown', e => { if (e.key === 'Escape') cancelEdit() })
      catInput.addEventListener('keydown',  e => { if (e.key === 'Escape') cancelEdit() })

      wrap.appendChild(nameInput); wrap.appendChild(catInput); wrap.appendChild(saveBtn)
      el.appendChild(wrap)
      nameInput.focus(); nameInput.select()
    })
    el.appendChild(editBtn)

    // Remove button (with undo)
    const rm = document.createElement('button')
    rm.className = 'remove-btn'
    rm.textContent = '✕'
    rm.title = 'Remove'
    rm.addEventListener('click', e => {
      e.stopPropagation()
      if (pendingDelete) {
        clearTimeout(pendingDelete.timer)
        window.hub.removeSource(pendingDelete.source.id)
        state.sources = state.sources.filter(s => s.id !== pendingDelete.source.id)
        state.items   = state.items.filter(i => i.sourceId !== pendingDelete.source.id)
        pendingDelete = null
      }
      if (activeFilter === source.id) { activeFilter = 'all'; updateMainHeader() }
      source._pendingDelete = true
      renderSidebar(); applyFilters()
      showUndoToast(`"${source.name}" removed`, () => {
        // undo
        delete source._pendingDelete
        pendingDelete = null
        renderSidebar(); renderCategoryPills(); applyFilters()
      }, async () => {
        // commit
        await window.hub.removeSource(source.id)
        state.sources = state.sources.filter(s => s.id !== source.id)
        state.items   = state.items.filter(i => i.sourceId !== source.id)
        pendingDelete = null
        renderSidebar(); renderCategoryPills(); applyFilters()
      })
    })
    el.appendChild(rm)
  }

  el.addEventListener('click', () => {
    if (el.classList.contains('editing')) return
    if (filter === 'manual-links') {
      activeFilter = 'all'
      activeCategory = 'manual'
      document.querySelectorAll('.pill').forEach(p => p.classList.remove('active'))
    } else {
      activeFilter = filter
      activeCategory = 'all'
    }
    document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'))
    el.classList.add('active')
    closeStats(); closeNotes()
    updateMainHeader()
    applyFilters()
  })

  return el
}

// ── Category pills ────────────────────────────────────────────────
function renderCategoryPills() {
  const container = document.getElementById('category-pills')
  container.innerHTML = ''

  const cats = [...new Set(state.sources.map(s => s.category || 'General'))]
  const labels = { all: 'All', youtube: 'YouTube', rss: 'RSS', manual: 'Links', saved: '★ Saved', later: '⏱ Later' }

  for (const cat of ['all', 'youtube', 'rss', ...cats, 'saved', 'later']) {
    const btn = makePill(labels[cat] ?? cat, () => {
      activeCategory = cat
      activeTag = ''
      document.querySelectorAll('.pill').forEach(p => p.classList.remove('active'))
      btn.classList.add('active')
      closeStats(); closeNotes()
      applyFilters()
    }, activeCategory === cat && !activeTag)
    container.appendChild(btn)
  }

  // Tag pills
  const allTags = [...new Set(state.items.flatMap(i => i.tags || []))]
  if (allTags.length > 0) {
    const sep = document.createElement('span')
    sep.className = 'pill-sep'
    sep.textContent = '/'
    container.appendChild(sep)

    for (const tag of allTags) {
      const btn = makePill('#' + tag, () => {
        activeTag = activeTag === tag ? '' : tag
        if (activeTag) {
          document.querySelectorAll('.pill').forEach(p => p.classList.remove('active'))
          btn.classList.add('active')
        } else {
          renderCategoryPills()
        }
        closeStats(); closeNotes()
        applyFilters()
      }, activeTag === tag)
      container.appendChild(btn)
    }
  }
}

function makePill(text, onClick, isActive) {
  const btn = document.createElement('button')
  btn.className = 'pill' + (isActive ? ' active' : '')
  btn.textContent = text
  btn.addEventListener('click', onClick)
  return btn
}

// ── Date bucket helpers ───────────────────────────────────────────
function dateBucket(dateStr) {
  if (!dateStr) return 'Earlier'
  const d   = new Date(dateStr)
  if (isNaN(d)) return 'Earlier'
  const now = new Date()
  const todayStr     = now.toDateString()
  const yest = new Date(now); yest.setDate(yest.getDate() - 1)
  const yestStr      = yest.toDateString()
  const weekAgo      = now - 7 * 864e5
  if (d.toDateString() === todayStr) return 'Today'
  if (d.toDateString() === yestStr)  return 'Yesterday'
  if (d >= weekAgo)                  return 'This week'
  return 'Earlier'
}

const BUCKET_ORDER = ['Today', 'Yesterday', 'This week', 'Earlier']

// ── Onboarding suggestions ────────────────────────────────────────
const SUGGESTIONS = [
  { name: 'Hacker News',   url: 'https://news.ycombinator.com/rss',                type: 'rss' },
  { name: 'CSS-Tricks',    url: 'https://css-tricks.com/feed/',                    type: 'rss' },
  { name: 'The Verge',     url: 'https://www.theverge.com/rss/index.xml',          type: 'rss' },
  { name: 'Smashing Mag',  url: 'https://www.smashingmagazine.com/feed/',          type: 'rss' },
  { name: 'Dev.to',        url: 'https://dev.to/feed',                             type: 'rss' },
]

async function addSuggestedSource({ name, url, type }) {
  const source = await window.hub.addSource({ name, url, feedUrl: '', type, category: 'General' })
  state.sources.push(source)
  renderSidebar(); renderCategoryPills()
  await doRefresh()
}

// ── Feed ──────────────────────────────────────────────────────────
function renderFeed() {
  const feed  = document.getElementById('feed')
  const empty = document.getElementById('empty-state')

  if (feedObserver) { feedObserver.disconnect(); feedObserver = null }
  Array.from(feed.children).forEach(c => { if (c.id !== 'empty-state') c.remove() })
  empty.hidden = true

  // Onboarding
  if (state.sources.filter(s => !s._pendingDelete).length === 0) {
    const ob = document.createElement('div')
    ob.id = 'onboarding'
    ob.innerHTML = '<h2>Welcome to Hub</h2><p>Add your first source, or start with one of these:</p>'
    const list = document.createElement('div')
    list.className = 'suggestion-list'
    SUGGESTIONS.forEach(s => {
      const btn = document.createElement('button')
      btn.className = 'suggestion-btn'
      btn.innerHTML = `<span>${s.name}</span><span>${s.url}</span>`
      btn.addEventListener('click', () => addSuggestedSource(s))
      list.appendChild(btn)
    })
    ob.appendChild(list)
    feed.appendChild(ob)
    return
  }

  const items = getFilteredItems()
  updateItemCount(items)

  if (items.length === 0) { empty.hidden = false; return }

  const slice = items.slice(0, visibleCount)

  // Group by date bucket (only when sorted by date)
  if (sortOrder !== 'source') {
    let currentBucket = null
    let cardIdx = 0
    slice.forEach(item => {
      const bucket = dateBucket(item.date)
      if (bucket !== currentBucket) {
        currentBucket = bucket
        const sep = document.createElement('div')
        sep.className = 'date-sep'
        sep.textContent = bucket
        feed.appendChild(sep)
      }
      const card = cardEl(item, cardIdx++)
      feed.appendChild(card)
    })
  } else {
    slice.forEach((item, i) => feed.appendChild(cardEl(item, i)))
  }

  if (visibleCount < items.length) attachSentinel(items)

  // Restore scroll position
  const key = activeFilter + '|' + activeCategory
  const savedScroll = scrollMemory.get(key)
  if (savedScroll) feed.scrollTop = savedScroll
}

function attachSentinel(items) {
  const feed = document.getElementById('feed')
  const sentinel = document.createElement('div')
  sentinel.className = 'sentinel'
  feed.appendChild(sentinel)

  feedObserver = new IntersectionObserver(entries => {
    if (!entries[0].isIntersecting) return
    feedObserver.disconnect()
    const from = visibleCount
    visibleCount = Math.min(visibleCount + 20, items.length)
    sentinel.remove()
    items.slice(from, visibleCount).forEach((item, i) => feed.appendChild(cardEl(item, i)))
    if (visibleCount < items.length) attachSentinel(items)
    else feedObserver = null
  }, { rootMargin: '200px' })

  feedObserver.observe(sentinel)
}

// ── Cards ─────────────────────────────────────────────────────────
function hashColor(str) {
  let h = 0
  for (let i = 0; i < str.length; i++) h = (h * 31 + str.charCodeAt(i)) >>> 0
  const hue = h % 360
  return `hsl(${hue}, 38%, 32%)`
}

function thumbPlaceholder(sourceName) {
  const div = document.createElement('div')
  div.className = 'thumb-initials'
  const words = (sourceName || '?').split(/\s+/)
  const initials = words.length > 1
    ? (words[0][0] + words[1][0]).toUpperCase()
    : (sourceName || '?').slice(0, 2).toUpperCase()
  div.textContent = initials
  div.style.background = hashColor(sourceName || '')
  return div
}

function cardEl(item, staggerIdx = 0) {
  const isCompact = viewMode === 'compact'
  const card = document.createElement('div')
  card.className = 'card' + (item.read ? ' read' : '') + (isCompact ? ' compact' : '')
  card.dataset.itemId = item.id
  card.style.setProperty('--i', Math.min(staggerIdx, 12))
  if (readerItem && readerItem.id === item.id) card.classList.add('active-reader')

  if (!isCompact) {
    if (item.thumbnail) {
      const img = document.createElement('img')
      img.className = 'card-thumb'
      img.src = item.thumbnail; img.alt = ''
      img.onerror = () => img.replaceWith(thumbPlaceholder(item.sourceName))
      card.appendChild(img)
    } else {
      card.appendChild(thumbPlaceholder(item.sourceName))
    }
  }

  const body = document.createElement('div')
  body.className = 'card-body'

  const title = document.createElement('div')
  title.className = 'card-title'
  title.textContent = item.title
  body.appendChild(title)

  const meta = document.createElement('div')
  meta.className = 'card-meta'

  const src = document.createElement('span')
  src.className = 'card-source'
  src.textContent = item.sourceName

  const type = document.createElement('span')
  const typeKey = item.sourceType === 'youtube' ? 'yt' : item.sourceType === 'manual' ? 'manual' : 'rss'
  type.className = 'card-type ' + typeKey
  type.textContent = item.sourceType === 'youtube' ? 'yt' : item.sourceType === 'manual' ? 'link' : 'rss'

  const date = document.createElement('span')
  date.textContent = formatDate(item.date)

  meta.appendChild(src); meta.appendChild(type); meta.appendChild(date)

  if (item.readMinutes && !isCompact) {
    const rt = document.createElement('span')
    rt.textContent = item.readMinutes + ' min'
    rt.title = 'Estimated read time'
    meta.appendChild(rt)
  }

  if (item.note) {
    const nd = document.createElement('span')
    nd.className = 'note-dot'; nd.textContent = '✎'; nd.title = item.note
    meta.appendChild(nd)
  }

  body.appendChild(meta)

  if (!isCompact && item.tags && item.tags.length > 0) {
    const tagsWrap = document.createElement('div')
    tagsWrap.className = 'card-tags'
    item.tags.forEach(t => {
      const chip = document.createElement('span')
      chip.className = 'tag-chip'; chip.textContent = '#' + t
      tagsWrap.appendChild(chip)
    })
    body.appendChild(tagsWrap)
  }

  card.appendChild(body)

  const btnWrap = document.createElement('div')
  btnWrap.style.cssText = 'display:flex;flex-direction:column;gap:2px;flex-shrink:0;align-self:flex-start'

  const bm = document.createElement('button')
  bm.className = 'bookmark-btn' + (item.saved ? ' saved' : '')
  bm.title = item.saved ? 'Unsave' : 'Save'
  bm.textContent = item.saved ? '★' : '☆'
  bm.addEventListener('click', async e => {
    e.stopPropagation()
    const updated = await window.hub.toggleSaved(item.id)
    if (!updated) return
    item.saved = updated.saved
    bm.className = 'bookmark-btn' + (item.saved ? ' saved' : '')
    bm.textContent = item.saved ? '★' : '☆'
    bm.title = item.saved ? 'Unsave' : 'Save'
    if (activeCategory === 'saved') applyFilters()
  })
  btnWrap.appendChild(bm)

  const lb = document.createElement('button')
  lb.className = 'later-btn' + (item.later ? ' on' : '')
  lb.title = item.later ? 'Remove from Later' : 'Read later'
  lb.textContent = '⏱'
  lb.addEventListener('click', async e => {
    e.stopPropagation()
    const updated = await window.hub.toggleLater(item.id)
    if (!updated) return
    item.later = updated.later
    lb.className = 'later-btn' + (item.later ? ' on' : '')
    lb.title = item.later ? 'Remove from Later' : 'Read later'
    if (activeCategory === 'later') applyFilters()
  })
  btnWrap.appendChild(lb)

  // Quick note button
  const qn = document.createElement('button')
  qn.className = 'quick-note-btn'
  qn.title = 'Quick note'
  qn.textContent = '✎'
  qn.addEventListener('click', e => {
    e.stopPropagation()
    openQuickNote(item, card.getBoundingClientRect())
  })
  btnWrap.appendChild(qn)

  card.appendChild(btnWrap)
  card.addEventListener('click', () => openReader(item))

  return card
}

function formatDate(dateStr) {
  if (!dateStr) return ''
  const d = new Date(dateStr)
  if (isNaN(d)) return ''
  const h = (Date.now() - d) / 3.6e6
  if (h < 1)   return 'just now'
  if (h < 24)  return `${Math.floor(h)}h ago`
  if (h < 168) return `${Math.floor(h / 24)}d ago`
  return d.toLocaleDateString('en', { month: 'short', day: 'numeric' })
}

// ── Reader panel ──────────────────────────────────────────────────
// Panel states: '' (closed) | 'open' (normal ~50%) | 'expanded' (~75%)

const EXPAND_ICON_OUT = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/><line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/></svg>`
const EXPAND_ICON_IN  = `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="4 14 10 14 10 20"/><polyline points="20 10 14 10 14 4"/><line x1="10" y1="14" x2="3" y2="21"/><line x1="21" y1="3" x2="14" y2="10"/></svg>`

function readerUrl(item) {
  if (!item.link) return 'about:blank'
  const m = item.link.match(/(?:youtube\.com\/watch\?.*v=|youtu\.be\/)([a-zA-Z0-9_-]{11})/)
  if (m) return `https://www.youtube.com/watch?v=${m[1]}`
  return item.link
}

function setReaderPanelState(state) {
  // state: 'open' | 'expanded' | '' (closed)
  const panel = document.getElementById('reader-panel')
  panel.classList.toggle('open',     state === 'open')
  panel.classList.toggle('expanded', state === 'expanded')
  document.getElementById('reader-expand').innerHTML =
    state === 'expanded' ? EXPAND_ICON_IN : EXPAND_ICON_OUT
  document.getElementById('reader-expand').title =
    state === 'expanded' ? 'Collapse panel' : 'Expand panel'
}

function showReaderFallback() {
  document.getElementById('reader-wv').hidden = true
  document.getElementById('reader-fallback').hidden = false
}

function hideReaderFallback() {
  document.getElementById('reader-wv').hidden = false
  document.getElementById('reader-fallback').hidden = true
}

function openReader(item) {
  if (readerItem) {
    document.querySelector(`[data-item-id="${readerItem.id}"]`)?.classList.remove('active-reader')
  }

  readerItem = item
  document.querySelector(`[data-item-id="${item.id}"]`)?.classList.add('active-reader')

  document.getElementById('reader-title').textContent = item.title
  document.getElementById('reader-note').value = item.note || ''
  document.getElementById('reader-tags').value = (item.tags || []).map(t => '#' + t).join(' ')
  updateTemplateBar()

  // Load note-specific tags
  window.hub.getNote(item.id).then(note => {
    document.getElementById('reader-note-tags').value = (note?.noteTags || []).map(t => '#' + t).join(' ')
  })

  // Reset reader mode and note mode for new item
  if (readerMode) {
    readerMode = false
    document.getElementById('reader-content').hidden = true
    document.getElementById('reader-mode-btn').classList.remove('active')
  }
  if (noteMode) {
    noteMode = false
    document.getElementById('note-editor-view').hidden = true
    document.getElementById('note-mode-btn').classList.remove('active')
  }
  hideReaderFallback()
  document.getElementById('highlight-tooltip').hidden = true
  document.getElementById('note-help-popover').hidden = true

  const modeBtn = document.getElementById('reader-mode-btn')
  modeBtn.disabled = item.sourceType !== 'rss'

  document.getElementById('reader-wv').src = readerUrl(item)

  // If already open/expanded, keep the current size; otherwise open normally
  const panel = document.getElementById('reader-panel')
  if (!panel.classList.contains('open') && !panel.classList.contains('expanded')) {
    setReaderPanelState('open')
  }

  if (!item.read) {
    window.hub.markRead(item.id)
    item.read = true
    document.querySelector(`[data-item-id="${item.id}"]`)?.classList.add('read')
    renderSidebar()
  }

  // Load backlinks & unlinked mentions
  loadBacklinks(item.id)
  loadUnlinkedMentions(item)

  // Render note preview if note has wikilinks
  if (item.note && item.note.includes('[[')) {
    window.hub.saveNote({ itemId: item.id, content: item.note }).then(result => {
      renderNotePreview(item.note, result?.resolvedLinks || [])
    })
  } else {
    document.getElementById('note-preview').hidden = true
  }
}

function closeReader() {
  document.querySelector(`[data-item-id="${readerItem?.id}"]`)?.classList.remove('active-reader')
  setReaderPanelState('')
  document.getElementById('reader-wv').src = 'about:blank'
  hideReaderFallback()
  document.getElementById('reader-content').hidden = true
  document.getElementById('note-editor-view').hidden = true
  document.getElementById('highlight-tooltip').hidden = true
  document.getElementById('note-help-popover').hidden = true
  if (readerMode) {
    readerMode = false
    document.getElementById('reader-mode-btn').classList.remove('active')
  }
  if (noteMode) {
    noteMode = false
    document.getElementById('note-mode-btn').classList.remove('active')
  }
  readerItem = null
}

// X-Frame-Options / blocked content fallback
document.getElementById('reader-wv').addEventListener('did-fail-load', e => {
  if (e.errorCode === -3) return  // aborted navigation — ignore
  if (!e.validatedURL || e.validatedURL === 'about:blank') return
  if (e.errorCode === -501) showReaderFallback()
})

document.getElementById('reader-back').addEventListener('click', closeReader)

document.getElementById('reader-expand').addEventListener('click', () => {
  const panel = document.getElementById('reader-panel')
  if (panel.classList.contains('expanded')) {
    setReaderPanelState('open')
  } else if (panel.classList.contains('open')) {
    setReaderPanelState('expanded')
  }
})

document.getElementById('reader-external').addEventListener('click', () => {
  if (readerItem) window.hub.openExternal(readerItem.link)
})

document.getElementById('reader-fallback-btn').addEventListener('click', () => {
  if (readerItem) window.hub.openExternal(readerItem.link)
})

// ── Reader mode ───────────────────────────────────────────────────
function renderReaderBlocks(blocks) {
  const container = document.getElementById('reader-content')
  container.innerHTML = ''
  if (!blocks || blocks.length === 0) {
    const p = document.createElement('p')
    p.className = 'reader-empty'
    p.textContent = 'No readable content found for this article.'
    container.appendChild(p)
    return
  }
  const body = document.createElement('div')
  body.className = 'reader-body'
  blocks.forEach(b => {
    const el = document.createElement(b.type === 'blockquote' ? 'blockquote' : b.type)
    el.textContent = b.text
    body.appendChild(el)
  })
  container.appendChild(body)
}

function highlightTextInReader(text) {
  const container = document.getElementById('reader-content')
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT)
  const nodes = []
  let node
  while ((node = walker.nextNode())) nodes.push(node)
  for (const tn of nodes) {
    const idx = tn.textContent.indexOf(text)
    if (idx < 0) continue
    const mark = document.createElement('mark')
    mark.textContent = text
    const frag = document.createDocumentFragment()
    if (idx > 0) frag.appendChild(document.createTextNode(tn.textContent.slice(0, idx)))
    frag.appendChild(mark)
    const after = tn.textContent.slice(idx + text.length)
    if (after) frag.appendChild(document.createTextNode(after))
    tn.parentNode.replaceChild(frag, tn)
    break
  }
}

document.getElementById('reader-mode-btn').addEventListener('click', async () => {
  if (!readerItem || readerItem.sourceType !== 'rss') return
  // Exit note mode if active
  if (noteMode) exitNoteMode()

  const btn = document.getElementById('reader-mode-btn')
  const wv  = document.getElementById('reader-wv')
  const rc  = document.getElementById('reader-content')

  if (readerMode) {
    readerMode = false
    rc.hidden = true
    wv.hidden = false
    btn.classList.remove('active')
    return
  }

  btn.disabled = true
  const result = await window.hub.fetchReadable(readerItem.link)
  btn.disabled = false

  if (result.error) { showToast('Reader unavailable: ' + result.error, 'error'); return }

  readerMode = true
  wv.hidden = true
  renderReaderBlocks(result.blocks)
  rc.hidden = false
  btn.classList.add('active')

  // Apply existing highlights for this item
  highlights.filter(h => h.itemId === readerItem.id)
             .forEach(h => highlightTextInReader(h.text))
})

// ── Note mode (full editor + live preview) ─────────────────────
function renderNoteEditorPreview(text) {
  const preview = document.getElementById('note-editor-preview')
  if (!text.trim()) { preview.innerHTML = ''; return }
  const wikilinkHtml = text.replace(/\[\[([^\]]+)\]\]/g, (m, wt) => {
    const lower = wt.toLowerCase()
    const allItems = state.items.filter(i => i.id !== readerItem?.id)
    let target = allItems.find(i => i.title.toLowerCase() === lower)
    if (!target) target = allItems.find(i => i.title.toLowerCase().includes(lower))
    if (!target) target = allItems.find(i => lower.includes(i.title.toLowerCase()) && i.title.length > 3)
    if (target) return `<span class="wikilink" data-id="${target.id}">${wt}</span>`
    return `<span class="wikilink broken">${wt}</span>`
  })
  preview.innerHTML = renderMarkdown(wikilinkHtml)
  preview.querySelectorAll('.wikilink[data-id]').forEach(el => {
    el.addEventListener('click', e => {
      e.stopPropagation()
      const targetItem = state.items.find(i => i.id === el.dataset.id)
      if (targetItem) openReader(targetItem)
    })
  })
}

function loadNoteEditorBacklinks(itemId) {
  window.hub.getBacklinks(itemId).then(result => {
    const container = document.getElementById('note-editor-backlinks')
    const backItems = result.items || result || []
    const backStandalone = result.standaloneNotes || []
    if (backItems.length === 0 && backStandalone.length === 0) { container.innerHTML = ''; return }
    container.innerHTML = '<div class="backlinks-title">Backlinks</div>'
    backItems.forEach(bl => {
      const el = document.createElement('div')
      el.className = 'backlink-item'
      el.innerHTML = `<span class="backlink-icon">←</span><span class="backlink-name">${bl.title}</span><span class="backlink-source">${bl.sourceName}</span>`
      el.addEventListener('click', () => openReader(bl))
      container.appendChild(el)
    })
    backStandalone.forEach(sn => {
      const el = document.createElement('div')
      el.className = 'backlink-item'
      el.innerHTML = `<span class="backlink-icon">←</span><span class="backlink-name">${sn.title || 'Standalone'}</span><span class="backlink-source">note</span>`
      el.addEventListener('click', () => openStandaloneEditor(sn.itemId))
      container.appendChild(el)
    })
  })
}

function loadNoteEditorUnlinked(item) {
  const container = document.getElementById('note-editor-unlinked')
  const noteText = (document.getElementById('note-editor-textarea').value || '').toLowerCase()
  if (!noteText) { container.innerHTML = ''; return }
  window.hub.getAllLinks().then(allLinks => {
    const linkedIds = new Set(allLinks.filter(l => l.fromItemId === item.id).map(l => l.toItemId))
    const mentions = state.items.filter(i =>
      i.id !== item.id && !linkedIds.has(i.id) &&
      i.title.length > 3 && noteText.includes(i.title.toLowerCase())
    )
    if (mentions.length === 0) { container.innerHTML = ''; return }
    container.innerHTML = '<div class="unlinked-title">Possible links</div>'
    mentions.forEach(m => {
      const el = document.createElement('div')
      el.className = 'unlinked-item'
      const label = document.createElement('span')
      label.className = 'unlinked-name'
      label.textContent = `[[${m.title}]]`
      const btn = document.createElement('button')
      btn.className = 'unlinked-link-btn'
      btn.textContent = 'Link'
      btn.addEventListener('click', e => {
        e.stopPropagation()
        const ta = document.getElementById('note-editor-textarea')
        ta.value += (ta.value ? '\n' : '') + `[[${m.title}]]`
        onNoteEditorInput()
      })
      el.appendChild(label); el.appendChild(btn)
      container.appendChild(el)
    })
  })
}

function enterNoteMode() {
  if (!readerItem) return
  // Exit reader mode if active
  if (readerMode) {
    readerMode = false
    document.getElementById('reader-content').hidden = true
    document.getElementById('reader-mode-btn').classList.remove('active')
  }
  noteMode = true
  // Hide webview/reader-content
  document.getElementById('reader-wv').hidden = true
  document.getElementById('reader-content').hidden = true
  document.getElementById('reader-fallback').hidden = true
  // Hide the footer (we move everything into the editor view)
  document.getElementById('reader-footer').hidden = true
  // Show editor view
  const ev = document.getElementById('note-editor-view')
  ev.hidden = false
  document.getElementById('note-mode-btn').classList.add('active')

  // Populate
  const ta = document.getElementById('note-editor-textarea')
  ta.value = readerItem.note || ''
  ta.focus()
  renderNoteEditorPreview(ta.value)

  // Template bar
  document.getElementById('note-editor-template-bar').hidden = ta.value.trim().length > 0

  // Note tags
  window.hub.getNote(readerItem.id).then(note => {
    document.getElementById('note-editor-tags').value = (note?.noteTags || []).map(t => '#' + t).join(' ')
  })

  // Backlinks + unlinked
  loadNoteEditorBacklinks(readerItem.id)
  loadNoteEditorUnlinked(readerItem)
}

function exitNoteMode() {
  noteMode = false
  document.getElementById('note-editor-view').hidden = true
  document.getElementById('note-mode-btn').classList.remove('active')
  document.getElementById('reader-footer').hidden = false
  // Restore webview (unless reader mode was active)
  if (readerMode) {
    document.getElementById('reader-content').hidden = false
  } else {
    document.getElementById('reader-wv').hidden = false
  }
}

let noteEditorDebounce = null

async function onNoteEditorInput() {
  const ta = document.getElementById('note-editor-textarea')
  handleSlashCommand(ta)
  // Live preview (immediate)
  renderNoteEditorPreview(ta.value)
  // Template bar
  document.getElementById('note-editor-template-bar').hidden = ta.value.trim().length > 0
  // Debounced save
  clearTimeout(noteEditorDebounce)
  noteEditorDebounce = setTimeout(async () => {
    if (!readerItem) return
    readerItem.note = ta.value
    const stateItem = state.items.find(i => i.id === readerItem.id)
    if (stateItem) stateItem.note = ta.value
    // Also sync footer textarea
    document.getElementById('reader-note').value = ta.value
    const result = await window.hub.saveNote({ itemId: readerItem.id, content: ta.value })
    // Refresh backlinks/unlinked in editor
    loadNoteEditorBacklinks(readerItem.id)
    loadNoteEditorUnlinked(readerItem)
    // Also update the footer sections
    renderNotePreview(ta.value, result?.resolvedLinks || [])
    await loadBacklinks(readerItem.id)
    // Card dot
    const card = document.querySelector(`[data-item-id="${readerItem.id}"]`)
    if (card) {
      const nd = card.querySelector('.note-dot')
      if (ta.value && !nd) {
        const meta = card.querySelector('.card-meta')
        const dot = document.createElement('span')
        dot.className = 'note-dot'; dot.textContent = '\u270e'; dot.title = ta.value
        meta?.appendChild(dot)
      } else if (!ta.value && nd) { nd.remove() }
      else if (nd) { nd.title = ta.value }
    }
    flashNoteSaved()
  }, 500)
}

document.getElementById('note-editor-textarea')?.addEventListener('input', onNoteEditorInput)

document.getElementById('note-editor-textarea')?.addEventListener('keydown', async e => {
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
    e.preventDefault()
    clearTimeout(noteEditorDebounce)
    const ta = document.getElementById('note-editor-textarea')
    readerItem.note = ta.value
    const stateItem = state.items.find(i => i.id === readerItem.id)
    if (stateItem) stateItem.note = ta.value
    document.getElementById('reader-note').value = ta.value
    await window.hub.saveNote({ itemId: readerItem.id, content: ta.value })
    flashNoteSaved()
  }
})

document.getElementById('note-editor-tags')?.addEventListener('blur', async () => {
  if (!readerItem) return
  const raw = document.getElementById('note-editor-tags').value
  const noteTags = raw.split(/[\s,]+/).map(t => t.replace(/^#/, '').trim()).filter(Boolean)
  document.getElementById('reader-note-tags').value = raw
  await window.hub.saveNote({ itemId: readerItem.id, content: readerItem.note || '', noteTags })
})

// Template buttons in editor view
document.querySelectorAll('.template-btn-editor').forEach(btn => {
  btn.addEventListener('click', () => {
    const tpl = NOTE_TEMPLATES[btn.dataset.tpl]
    if (!tpl) return
    const ta = document.getElementById('note-editor-textarea')
    ta.value = tpl
    ta.focus()
    document.getElementById('note-editor-template-bar').hidden = true
    onNoteEditorInput()
  })
})

// Note mode toggle
document.getElementById('note-mode-btn')?.addEventListener('click', () => {
  if (!readerItem) return
  if (noteMode) exitNoteMode()
  else enterNoteMode()
})

// Help popover toggle
document.getElementById('note-help-btn')?.addEventListener('click', () => {
  const pop = document.getElementById('note-help-popover')
  pop.hidden = !pop.hidden
})

// Close help on click outside
document.addEventListener('mousedown', e => {
  const pop = document.getElementById('note-help-popover')
  if (!pop.hidden && !pop.contains(e.target) && e.target.id !== 'note-help-btn' && !e.target.closest('#note-help-btn')) {
    pop.hidden = true
  }
})

// Text selection → highlight tooltip
document.getElementById('reader-content').addEventListener('mouseup', () => {
  const sel = window.getSelection()
  const tip = document.getElementById('highlight-tooltip')
  if (!sel || sel.isCollapsed) { tip.hidden = true; pendingHighlightText = ''; return }
  const text = sel.toString().trim()
  if (text.length < 3) { tip.hidden = true; pendingHighlightText = ''; return }
  pendingHighlightText = text
  const range = sel.getRangeAt(0)
  const rect  = range.getBoundingClientRect()
  const panel = document.getElementById('reader-panel').getBoundingClientRect()
  tip.hidden = false
  tip.style.top  = (rect.bottom - panel.top + 6) + 'px'
  tip.style.left = Math.max(0, rect.left - panel.left) + 'px'
})

document.getElementById('highlight-save-btn').addEventListener('mousedown', e => {
  e.preventDefault()  // prevent selection from collapsing
})

document.getElementById('highlight-save-btn').addEventListener('click', async () => {
  if (!pendingHighlightText || !readerItem) return
  const text = pendingHighlightText
  pendingHighlightText = ''
  highlights = await window.hub.saveHighlight({ itemId: readerItem.id, text })
  highlightTextInReader(text)
  window.getSelection()?.removeAllRanges()
  document.getElementById('highlight-tooltip').hidden = true
})

document.addEventListener('mousedown', e => {
  if (!document.getElementById('highlight-tooltip').contains(e.target)) {
    document.getElementById('highlight-tooltip').hidden = true
  }
})

let noteDebounceTimer = null

async function persistNote() {
  if (!readerItem) return
  const note = document.getElementById('reader-note').value
  readerItem.note = note
  const stateItem = state.items.find(i => i.id === readerItem.id)
  if (stateItem) stateItem.note = note
  const result = await window.hub.saveNote({ itemId: readerItem.id, content: note })
  // Refresh note indicator on the card
  const card = document.querySelector(`[data-item-id="${readerItem.id}"]`)
  if (card) {
    const nd = card.querySelector('.note-dot')
    if (note && !nd) {
      const meta = card.querySelector('.card-meta')
      const dot = document.createElement('span')
      dot.className = 'note-dot'; dot.textContent = '✎'; dot.title = note
      meta.appendChild(dot)
    } else if (!note && nd) {
      nd.remove()
    } else if (nd) {
      nd.title = note
    }
  }
  // Update note preview with rendered wikilinks
  renderNotePreview(note, result?.resolvedLinks || [])
  // Refresh backlinks for current item
  await loadBacklinks(readerItem.id)
  flashNoteSaved()
}

function flashNoteSaved() {
  const status = document.getElementById('note-save-status')
  status.textContent = 'saved ✓'
  status.classList.add('visible')
  clearTimeout(status._t)
  status._t = setTimeout(() => status.classList.remove('visible'), 1800)
}

// Debounced note saving
document.getElementById('reader-note').addEventListener('input', () => {
  const textarea = document.getElementById('reader-note')
  // Check for slash commands
  if (handleSlashCommand(textarea)) return
  updateTemplateBar()
  // Sync to editor textarea if note mode is open
  if (noteMode) {
    document.getElementById('note-editor-textarea').value = textarea.value
    renderNoteEditorPreview(textarea.value)
  }
  clearTimeout(noteDebounceTimer)
  noteDebounceTimer = setTimeout(() => persistNote(), 500)
})

document.getElementById('reader-note').addEventListener('blur', () => {
  clearTimeout(noteDebounceTimer)
  persistNote()
})

document.getElementById('reader-note').addEventListener('keydown', async e => {
  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
    e.preventDefault()
    clearTimeout(noteDebounceTimer)
    await persistNote()
  }
})

// ── Wikilink rendering in note preview ───────────────────────────────
function renderNotePreview(noteText, resolvedLinks) {
  const preview = document.getElementById('note-preview')
  if (!noteText || !noteText.includes('[[')) {
    preview.hidden = true
    return
  }
  preview.hidden = false
  preview.innerHTML = ''
  // Build resolved map: fromItemId->toItemId items
  const resolvedMap = new Map()
  for (const rl of resolvedLinks) {
    const target = state.items.find(i => i.id === rl.toItemId)
    if (target) resolvedMap.set(rl.toItemId, target)
  }

  // Render note with markdown + wikilinks
  // First resolve wikilinks in raw text, then render markdown
  const wikilinkHtml = noteText.replace(/\[\[([^\]]+)\]\]/g, (match, text) => {
    const lower = text.toLowerCase()
    const allItems = state.items.filter(i => i.id !== readerItem?.id)
    let target = allItems.find(i => i.title.toLowerCase() === lower)
    if (!target) target = allItems.find(i => i.title.toLowerCase().includes(lower))
    if (!target) target = allItems.find(i => lower.includes(i.title.toLowerCase()) && i.title.length > 3)
    if (target) {
      return `<span class="wikilink" data-id="${target.id}">${text}</span>`
    }
    return `<span class="wikilink broken">${text}</span>`
  })
  preview.innerHTML = renderMarkdown(wikilinkHtml)

  // Click handlers for wikilinks
  preview.querySelectorAll('.wikilink[data-id]').forEach(el => {
    el.addEventListener('click', e => {
      e.stopPropagation()
      const targetItem = state.items.find(i => i.id === el.dataset.id)
      if (targetItem) openReader(targetItem)
    })
  })
}

// ── Backlinks ─────────────────────────────────────────────────────
async function loadBacklinks(itemId) {
  const result = await window.hub.getBacklinks(itemId)
  const section = document.getElementById('backlinks-section')
  const list = document.getElementById('backlinks-list')
  list.innerHTML = ''
  const backItems = result.items || result || []
  const backStandalone = result.standaloneNotes || []
  if (backItems.length === 0 && backStandalone.length === 0) {
    section.hidden = true
    return
  }
  section.hidden = false
  backItems.forEach(bl => {
    const el = document.createElement('div')
    el.className = 'backlink-item'
    el.innerHTML = `<span class="backlink-icon">←</span><span class="backlink-name">${bl.title}</span><span class="backlink-source">${bl.sourceName}</span>`
    el.addEventListener('click', () => openReader(bl))
    list.appendChild(el)
  })
  backStandalone.forEach(sn => {
    const el = document.createElement('div')
    el.className = 'backlink-item'
    el.innerHTML = `<span class="backlink-icon">←</span><span class="backlink-name">${sn.title || 'Standalone'}</span><span class="backlink-source">note</span>`
    el.addEventListener('click', () => openStandaloneEditor(sn.itemId))
    list.appendChild(el)
  })
}

// ── Unlinked mentions ────────────────────────────────────────────
async function loadUnlinkedMentions(item) {
  const section = document.getElementById('unlinked-section')
  const list = document.getElementById('unlinked-list')
  list.innerHTML = ''
  const noteText = (item.note || '').toLowerCase()
  if (!noteText) { section.hidden = true; return }

  const allLinks = await window.hub.getAllLinks()
  const linkedIds = new Set(allLinks.filter(l => l.fromItemId === item.id).map(l => l.toItemId))

  // Find items whose title appears in this item's note but aren't linked yet
  const mentions = state.items.filter(i =>
    i.id !== item.id && !linkedIds.has(i.id) &&
    i.title.length > 3 && noteText.includes(i.title.toLowerCase())
  )

  if (mentions.length === 0) { section.hidden = true; return }
  section.hidden = false

  mentions.forEach(m => {
    const el = document.createElement('div')
    el.className = 'unlinked-item'
    const label = document.createElement('span')
    label.className = 'unlinked-name'
    label.textContent = `[[${m.title}]]`
    const btn = document.createElement('button')
    btn.className = 'unlinked-link-btn'
    btn.textContent = 'Link'
    btn.addEventListener('click', async e => {
      e.stopPropagation()
      const textarea = document.getElementById('reader-note')
      textarea.value += (textarea.value ? '\n' : '') + `[[${m.title}]]`
      clearTimeout(noteDebounceTimer)
      await persistNote()
      await loadUnlinkedMentions(item)
    })
    el.appendChild(label)
    el.appendChild(btn)
    list.appendChild(el)
  })
}

// ── Markdown rendering (basic) ──────────────────────────────────────
function renderMarkdown(text) {
  return text
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/`(.+?)`/g, '<code>$1</code>')
    .replace(/^### (.+)$/gm, '<h3>$1</h3>')
    .replace(/^## (.+)$/gm, '<h2>$1</h2>')
    .replace(/^# (.+)$/gm, '<h1>$1</h1>')
    .replace(/^> (.+)$/gm, '<blockquote>$1</blockquote>')
    .replace(/^- (.+)$/gm, '<li>$1</li>')
    .replace(/(<li>.*<\/li>)/gs, '<ul>$1</ul>')
    .replace(/\n/g, '<br>')
}

// ── Note templates ──────────────────────────────────────────────────
const NOTE_TEMPLATES = {
  summary: '## Summary\n\n- Key point:\n- Key point:\n- \n\n## Takeaway\n\n',
  'key-ideas': '## Key Ideas\n\n1. \n2. \n3. \n\n## Connections\n\n- [[]]\n',
  todos: '## Action Items\n\n- [ ] \n- [ ] \n- [ ] \n\n## Notes\n\n',
  review: '## Rating\n\n★★★☆☆\n\n## What I liked\n\n- \n\n## What I didn\'t\n\n- \n\n## Key quote\n\n> \n',
}

// Show template bar when note is empty
function updateTemplateBar() {
  const note = document.getElementById('reader-note').value
  const bar = document.getElementById('note-template-bar')
  bar.hidden = note.trim().length > 0
}

document.querySelectorAll('.template-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    const tpl = NOTE_TEMPLATES[btn.dataset.tpl]
    if (!tpl) return
    const textarea = document.getElementById('reader-note')
    textarea.value = tpl
    textarea.focus()
    document.getElementById('note-template-bar').hidden = true
    clearTimeout(noteDebounceTimer)
    noteDebounceTimer = setTimeout(() => persistNote(), 500)
  })
})

// ── Slash commands ──────────────────────────────────────────────────
function handleSlashCommand(textarea) {
  const val = textarea.value
  const pos = textarea.selectionStart
  const lineStart = val.lastIndexOf('\n', pos - 1) + 1
  const line = val.substring(lineStart, pos)

  if (line === '/date') {
    const date = new Date().toLocaleDateString('en', { year: 'numeric', month: 'short', day: 'numeric' })
    textarea.value = val.substring(0, lineStart) + date + val.substring(pos)
    textarea.selectionStart = textarea.selectionEnd = lineStart + date.length
    return true
  }
  if (line === '/link') {
    // Insert [[ ]] and position cursor inside
    textarea.value = val.substring(0, lineStart) + '[[]]' + val.substring(pos)
    textarea.selectionStart = textarea.selectionEnd = lineStart + 2
    return true
  }
  if (line === '/template') {
    document.getElementById('note-template-bar').hidden = false
    textarea.value = val.substring(0, lineStart) + val.substring(pos)
    textarea.selectionStart = textarea.selectionEnd = lineStart
    return true
  }
  return false
}

// ── Cite from reader to note ────────────────────────────────────────
document.getElementById('highlight-cite-btn')?.addEventListener('mousedown', e => {
  e.preventDefault()
})

document.getElementById('highlight-cite-btn')?.addEventListener('click', () => {
  if (!pendingHighlightText || !readerItem) return
  const textarea = document.getElementById('reader-note')
  const cite = `\n> ${pendingHighlightText}\n`
  textarea.value += cite
  pendingHighlightText = ''
  window.getSelection()?.removeAllRanges()
  document.getElementById('highlight-tooltip').hidden = true
  clearTimeout(noteDebounceTimer)
  noteDebounceTimer = setTimeout(() => persistNote(), 500)
  textarea.focus()
})

// ── Quick note from card ────────────────────────────────────────────
let quickNoteItem = null

function openQuickNote(item, rect) {
  quickNoteItem = item
  const pop = document.getElementById('quick-note-popover')
  pop.hidden = false
  pop.style.top = (rect.bottom + 4) + 'px'
  pop.style.left = Math.max(8, rect.left) + 'px'
  const textarea = document.getElementById('quick-note-textarea')
  textarea.value = item.note || ''
  textarea.focus()
}

document.getElementById('quick-note-save')?.addEventListener('click', async () => {
  if (!quickNoteItem) return
  const content = document.getElementById('quick-note-textarea').value
  quickNoteItem.note = content
  const stateItem = state.items.find(i => i.id === quickNoteItem.id)
  if (stateItem) stateItem.note = content
  await window.hub.saveNote({ itemId: quickNoteItem.id, content })
  document.getElementById('quick-note-popover').hidden = true
  quickNoteItem = null
  renderFeed()
})

document.getElementById('quick-note-cancel')?.addEventListener('click', () => {
  document.getElementById('quick-note-popover').hidden = true
  quickNoteItem = null
})

// ── Note-specific tags in reader ────────────────────────────────────
document.getElementById('reader-note-tags')?.addEventListener('blur', async () => {
  if (!readerItem) return
  const raw = document.getElementById('reader-note-tags').value
  const noteTags = raw.split(/[\s,]+/).map(t => t.replace(/^#/, '').trim()).filter(Boolean)
  await window.hub.saveNote({ itemId: readerItem.id, content: readerItem.note || '', noteTags })
})

document.getElementById('reader-tags').addEventListener('blur', async () => {
  if (!readerItem) return
  const raw = document.getElementById('reader-tags').value
  const tags = raw.split(/[\s,]+/).map(t => t.replace(/^#/, '').trim()).filter(Boolean)
  readerItem.tags = tags
  const stateItem = state.items.find(i => i.id === readerItem.id)
  if (stateItem) stateItem.tags = tags
  await window.hub.saveTags(readerItem.id, tags)
  renderCategoryPills()
})

// ── Notes panel (enhanced) ────────────────────────────────────────
async function openNotes() {
  closeStats()
  const panel = document.getElementById('notes-panel')
  panel.hidden = false
  document.getElementById('notes-search').value = notesSearchQuery
  await renderNotesContent()
}

async function renderNotesContent() {
  const content = document.getElementById('notes-content')
  content.innerHTML = ''

  const allNotes = await window.hub.getAllNotes()
  const allLinks = await window.hub.getAllLinks()

  let entries = []

  if (notesFilter === 'standalone') {
    entries = allNotes.filter(n => n.itemId.startsWith('standalone-')).map(n => ({
      type: 'standalone', note: n,
      title: n.title || 'Untitled', source: 'note', date: n.updatedAt || n.createdAt,
      body: n.content, noteId: n.itemId, noteTags: n.noteTags || [],
    }))
  } else if (notesFilter === 'no-note') {
    const noted = new Set(allNotes.filter(n => n.content?.trim()).map(n => n.itemId))
    entries = state.items.filter(i => !noted.has(i.id) && !(i.note?.trim())).map(i => ({
      type: 'item', item: i, title: i.title, source: i.sourceName, date: i.date,
      body: '(no note)', noteId: null, noteTags: [],
    }))
  } else {
    const itemsWithNotes = state.items.filter(i => i.note && i.note.trim())
    const noteMap = new Map(allNotes.map(n => [n.itemId, n]))

    if (notesFilter !== 'has-note') {
      allNotes.filter(n => n.itemId.startsWith('standalone-')).forEach(n => {
        entries.push({
          type: 'standalone', note: n,
          title: n.title || 'Untitled', source: 'note',
          date: n.updatedAt || n.createdAt,
          body: n.content, noteId: n.itemId, noteTags: n.noteTags || [],
        })
      })
    }

    itemsWithNotes.forEach(item => {
      const noteObj = noteMap.get(item.id)
      entries.push({
        type: 'item', item, title: item.title, source: item.sourceName,
        date: notesFilter === 'timeline' ? (noteObj?.updatedAt || noteObj?.createdAt || item.date) : item.date,
        body: item.note, noteId: item.id, noteTags: noteObj?.noteTags || [],
      })
    })

    if (notesFilter === 'orphan') {
      const linkedIds = new Set()
      allLinks.forEach(l => { linkedIds.add(l.fromItemId); linkedIds.add(l.toItemId) })
      entries = entries.filter(e => e.noteId && !linkedIds.has(e.noteId))
    }
  }

  if (notesSearchQuery) {
    const q = notesSearchQuery.toLowerCase()
    entries = entries.filter(e =>
      (e.title || '').toLowerCase().includes(q) ||
      (e.body || '').toLowerCase().includes(q) ||
      (e.noteTags || []).some(t => t.toLowerCase().includes(q))
    )
  }

  entries.sort((a, b) => (b.date ? new Date(b.date) : 0) - (a.date ? new Date(a.date) : 0))

  if (entries.length === 0) {
    const empty = document.createElement('p')
    empty.className = 'notes-empty'
    empty.textContent = notesFilter === 'no-note' ? 'All items have notes! \uD83C\uDF89'
      : notesFilter === 'orphan' ? 'No orphan notes. All notes are connected.'
      : 'No notes found.'
    content.appendChild(empty)
    return
  }

  entries.forEach(entry => {
    const el = document.createElement('div')
    el.className = 'note-entry' + (entry.type === 'standalone' ? ' standalone' : '')

    const header = document.createElement('div')
    header.className = 'note-entry-header'
    const title = document.createElement('span')
    title.className = 'note-entry-title'
    title.textContent = entry.title; title.title = entry.title
    const src = document.createElement('span')
    src.className = 'note-entry-source'
    src.textContent = entry.source
    const date = document.createElement('span')
    date.className = 'note-entry-date'
    date.textContent = formatDate(entry.date)
    header.appendChild(title); header.appendChild(src); header.appendChild(date)

    const body = document.createElement('div')
    body.className = 'note-entry-body'
    body.innerHTML = renderMarkdown(entry.body || '')

    el.appendChild(header); el.appendChild(body)

    if (entry.noteTags && entry.noteTags.length) {
      const tagsEl = document.createElement('div')
      tagsEl.className = 'note-entry-tags'
      entry.noteTags.forEach(t => {
        const chip = document.createElement('span')
        chip.className = 'note-tag-chip'
        chip.textContent = '#' + t
        tagsEl.appendChild(chip)
      })
      el.appendChild(tagsEl)
    }

    el.addEventListener('click', () => {
      if (entry.type === 'standalone') { closeNotes(); openStandaloneEditor(entry.noteId) }
      else if (entry.item) { closeNotes(); openReader(entry.item) }
    })
    content.appendChild(el)
  })
}

function closeNotes() {
  document.getElementById('notes-panel').hidden = true
}

document.getElementById('notes-btn').addEventListener('click', () => {
  if (!document.getElementById('notes-panel').hidden) { closeNotes(); return }
  openNotes()
})
document.getElementById('notes-close').addEventListener('click', closeNotes)

document.getElementById('notes-search')?.addEventListener('input', e => {
  notesSearchQuery = e.target.value.trim()
  renderNotesContent()
})

document.querySelectorAll('.notes-filter-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    notesFilter = btn.dataset.filter
    document.querySelectorAll('.notes-filter-btn').forEach(b => b.classList.remove('active'))
    btn.classList.add('active')
    renderNotesContent()
  })
})

document.getElementById('notes-new-standalone')?.addEventListener('click', () => {
  closeNotes()
  openStandaloneEditor(null)
})

// ── Standalone note editor ────────────────────────────────────────
let standaloneDebounce = null

async function openStandaloneEditor(noteId) {
  const overlay = document.getElementById('standalone-overlay')
  overlay.hidden = false
  activeStandaloneId = noteId

  const titleEl = document.getElementById('standalone-title')
  const contentEl = document.getElementById('standalone-content')
  const tagsEl = document.getElementById('standalone-tags')
  document.getElementById('standalone-preview').innerHTML = ''

  if (noteId) {
    const note = await window.hub.getNote(noteId)
    if (note) {
      titleEl.value = note.title || ''
      contentEl.value = note.content || ''
      tagsEl.value = (note.noteTags || []).map(t => '#' + t).join(' ')
      renderStandalonePreview(note.content || '')
    }
  } else {
    titleEl.value = ''; contentEl.value = ''; tagsEl.value = ''
  }
  titleEl.focus()
}

function renderStandalonePreview(text) {
  const preview = document.getElementById('standalone-preview')
  if (!text.trim()) { preview.innerHTML = ''; return }
  const wikilinkHtml = text.replace(/\[\[([^\]]+)\]\]/g, (m, wt) => {
    const lower = wt.toLowerCase()
    let target = state.items.find(i => i.title.toLowerCase() === lower)
    if (!target) target = state.items.find(i => i.title.toLowerCase().includes(lower))
    if (!target) target = state.items.find(i => lower.includes(i.title.toLowerCase()) && i.title.length > 3)
    if (target) return `<span class="wikilink" data-id="${target.id}">${wt}</span>`
    return `<span class="wikilink broken">${wt}</span>`
  })
  preview.innerHTML = renderMarkdown(wikilinkHtml)
  preview.querySelectorAll('.wikilink[data-id]').forEach(el => {
    el.addEventListener('click', e => {
      e.stopPropagation()
      const targetItem = state.items.find(i => i.id === el.dataset.id)
      if (targetItem) { closeStandaloneEditor(); openReader(targetItem) }
    })
  })
}

async function persistStandalone() {
  const title = document.getElementById('standalone-title').value.trim()
  const content = document.getElementById('standalone-content').value
  const raw = document.getElementById('standalone-tags').value
  const noteTags = raw.split(/[\s,]+/).map(t => t.replace(/^#/, '').trim()).filter(Boolean)
  const result = await window.hub.saveStandaloneNote({
    id: activeStandaloneId, title: title || 'Untitled', content, noteTags
  })
  if (result.noteId && !activeStandaloneId) activeStandaloneId = result.noteId
  renderStandalonePreview(content)
  const status = document.getElementById('standalone-save-status')
  status.textContent = 'saved \u2713'
  status.classList.add('visible')
  clearTimeout(status._t)
  status._t = setTimeout(() => status.classList.remove('visible'), 1800)
}

function closeStandaloneEditor() {
  document.getElementById('standalone-overlay').hidden = true
  activeStandaloneId = null
}

document.getElementById('standalone-content')?.addEventListener('input', () => {
  handleSlashCommand(document.getElementById('standalone-content'))
  clearTimeout(standaloneDebounce)
  standaloneDebounce = setTimeout(() => persistStandalone(), 500)
})
document.getElementById('standalone-title')?.addEventListener('blur', () => {
  clearTimeout(standaloneDebounce); persistStandalone()
})
document.getElementById('standalone-tags')?.addEventListener('blur', () => {
  clearTimeout(standaloneDebounce); persistStandalone()
})
document.getElementById('standalone-close')?.addEventListener('click', closeStandaloneEditor)
document.getElementById('standalone-overlay')?.addEventListener('click', e => {
  if (e.target === document.getElementById('standalone-overlay')) closeStandaloneEditor()
})
document.getElementById('standalone-delete')?.addEventListener('click', async () => {
  if (!activeStandaloneId) return
  await window.hub.deleteStandaloneNote(activeStandaloneId)
  closeStandaloneEditor()
  showToast('Note deleted.', 'info')
})

// ── Stats ─────────────────────────────────────────────────────────
function closeStats() {
  document.getElementById('stats-panel').hidden = true
}

async function openStats() {
  const panel = document.getElementById('stats-panel')
  const content = document.getElementById('stats-content')
  panel.hidden = false
  content.innerHTML = '<div style="color:var(--text-3);font-size:12px;padding:12px 0">Loading…</div>'

  const s = await window.hub.getStats()
  content.innerHTML = ''

  const row = document.createElement('div')
  row.className = 'stats-row'
  ;[['Today', s.readToday], ['This week', s.readThisWeek], ['Total read', s.totalRead], ['Saved', s.totalSaved], ['Streak', s.streak + (s.streak === 1 ? ' day' : ' days')]].forEach(([label, val]) => {
    const card = document.createElement('div')
    card.className = 'stat-card'
    card.innerHTML = `<div class="stat-val">${val}</div><div class="stat-label">${label}</div>`
    row.appendChild(card)
  })
  content.appendChild(row)

  if (s.topSources.length > 0) {
    const sec = document.createElement('div')
    sec.className = 'stats-section'
    sec.innerHTML = '<div class="stats-section-title">Top sources this week</div>'
    const max = s.topSources[0].count
    s.topSources.forEach(({ name, count }) => {
      const pct = Math.round(count / max * 100)
      const r = document.createElement('div')
      r.className = 'stat-bar-row'
      r.innerHTML = `<span class="stat-bar-label">${name}</span><div class="stat-bar-track"><div class="stat-bar-fill" style="width:${pct}%"></div></div><span class="stat-bar-count">${count}</span>`
      sec.appendChild(r)
    })
    content.appendChild(sec)
  }
}

document.getElementById('stats-btn').addEventListener('click', openStats)
document.getElementById('stats-close').addEventListener('click', closeStats)

// ── Focus mode ────────────────────────────────────────────────────
document.getElementById('focus-btn').addEventListener('click', () => document.body.classList.toggle('focus'))

// ── Sidebar peek (focus mode) ──────────────────────────────────────
document.getElementById('sidebar-trigger').addEventListener('mouseenter', () => {
  if (document.body.classList.contains('focus')) {
    document.getElementById('sidebar').classList.add('peek')
  }
})
document.getElementById('sidebar').addEventListener('mouseleave', () => {
  document.getElementById('sidebar').classList.remove('peek')
})

// ── Export / Import ───────────────────────────────────────────────
document.getElementById('export-btn').addEventListener('click', () => {
  const menu = document.getElementById('export-menu')
  menu.hidden = !menu.hidden
})

document.getElementById('export-menu').addEventListener('click', async e => {
  const action = e.target.closest('[data-action]')?.dataset.action
  if (!action) return
  document.getElementById('export-menu').hidden = true

  if (action === 'export-md') {
    const res = await window.hub.exportMarkdown()
    if (res.error) showToast(res.error, 'error')
    else if (res.ok) showToast('Exported to Markdown.', 'info')
  } else if (action === 'export-opml') {
    const res = await window.hub.exportOpml()
    if (res.ok) showToast('Exported to OPML.', 'info')
  } else if (action === 'import-opml') {
    const res = await window.hub.importOpml()
    if (res.error) { showToast(res.error, 'error'); return }
    if (res.cancelled) return
    showToast(`Imported ${res.added} of ${res.total} feeds.`, 'info')
    state = await window.hub.getData()
    renderSidebar()
    renderCategoryPills()
    await doRefresh()
  }
})

// ── Header helpers ────────────────────────────────────────────────
function updateMainHeader() {
  const titleEl = document.getElementById('current-title')
  const markBtn = document.getElementById('mark-all-read-btn')
  if (activeFilter === 'all' || activeFilter === 'manual-links') {
    titleEl.textContent = activeFilter === 'manual-links' ? 'Links' : 'All'
    markBtn.hidden = true
  } else {
    const src = state.sources.find(s => s.id === activeFilter)
    titleEl.textContent = src ? src.name : 'Feed'
    markBtn.hidden = false
  }
}

function updateItemCount(items) {
  const el = document.getElementById('item-count')
  const unread = items.filter(i => !i.read).length
  el.textContent = unread > 0 ? `${items.length} items · ${unread} unread` : `${items.length} items`
}

// ── View mode ─────────────────────────────────────────────────────
const ICON_LIST = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><line x1="3" y1="6" x2="21" y2="6"/><line x1="3" y1="12" x2="21" y2="12"/><line x1="3" y1="18" x2="21" y2="18"/></svg>`
const ICON_GRID = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/></svg>`

function applyViewMode() {
  const feed = document.getElementById('feed')
  const btn  = document.getElementById('view-toggle')
  if (viewMode === 'compact') {
    feed.classList.add('compact'); btn.innerHTML = ICON_GRID; btn.title = 'Expanded view'
  } else {
    feed.classList.remove('compact'); btn.innerHTML = ICON_LIST; btn.title = 'Compact view'
  }
}

document.getElementById('view-toggle').addEventListener('click', () => {
  viewMode = viewMode === 'expanded' ? 'compact' : 'expanded'
  localStorage.setItem('hub-view', viewMode)
  applyViewMode(); applyFilters()
})

// ── Hide read ─────────────────────────────────────────────────────
function applyHideReadBtn() {
  document.getElementById('hide-read-btn').classList.toggle('active', hideRead)
}
document.getElementById('hide-read-btn').addEventListener('click', () => {
  hideRead = !hideRead
  localStorage.setItem('hub-hide-read', hideRead)
  applyHideReadBtn()
  applyFilters()
})

// ── Sort ──────────────────────────────────────────────────────────
document.getElementById('sort-select').addEventListener('change', e => { sortOrder = e.target.value; applyFilters() })

// ── Age filter ────────────────────────────────────────────────────
document.getElementById('age-filter').addEventListener('change', e => { ageFilter = parseInt(e.target.value); applyFilters() })

// ── Auto-refresh interval ─────────────────────────────────────────
document.getElementById('refresh-interval').addEventListener('change', async e => {
  await window.hub.saveSettings({ refreshInterval: parseInt(e.target.value) })
})

// ── Search ────────────────────────────────────────────────────────
document.getElementById('search-input').addEventListener('input', e => { searchQuery = e.target.value; applyFilters() })

// ── Mark all read ─────────────────────────────────────────────────
document.getElementById('mark-all-read-btn').addEventListener('click', async () => {
  if (activeFilter === 'all') return
  await window.hub.markAllRead(activeFilter)
  state.items.filter(i => i.sourceId === activeFilter).forEach(i => { i.read = true })
  renderSidebar(); renderFeed()
})

// ── Refresh ───────────────────────────────────────────────────────
async function doRefresh() {
  if (state.sources.length === 0) return
  const btn = document.getElementById('refresh-btn')
  btn.classList.add('spinning')
  const { items, errors } = await window.hub.refreshAll()
  state.items = items
  btn.classList.remove('spinning')
  renderSidebar(); renderCategoryPills(); applyFilters()
  updateStatusBar(); setLastRefresh()
  if (errors.length > 0) {
    showToast(`Failed to fetch ${errors.length} source${errors.length > 1 ? 's' : ''}:\n` + errors.map(e => `· ${e.name}: ${e.error}`).join('\n'), 'error')
  }
}

document.getElementById('refresh-btn').addEventListener('click', doRefresh)

// ── Modal ─────────────────────────────────────────────────────────
let selectedType = 'youtube'

document.getElementById('add-btn').addEventListener('click', openModal)
document.getElementById('modal-close').addEventListener('click', closeModal)
document.getElementById('modal-overlay').addEventListener('click', e => {
  if (e.target === document.getElementById('modal-overlay')) closeModal()
})

document.querySelectorAll('.type-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    selectedType = btn.dataset.type
    document.querySelectorAll('.type-btn').forEach(b => b.classList.remove('active'))
    btn.classList.add('active')
    updateModalFields()
  })
})

function updateModalFields() {
  const urlLabel  = document.getElementById('url-label')
  const nameLabel = document.getElementById('name-label')
  const inputUrl  = document.getElementById('input-url')
  const fieldCat  = document.getElementById('field-category')
  const submitBtn = document.getElementById('modal-submit')
  const modalTitle = document.getElementById('modal-title')

  if (selectedType === 'youtube') {
    urlLabel.textContent = 'Channel URL'
    inputUrl.placeholder = 'https://youtube.com/@fireship'
    fieldCat.hidden = false; nameLabel.textContent = 'Name'
    submitBtn.textContent = 'Add source'; modalTitle.textContent = 'Add source'
  } else if (selectedType === 'rss') {
    urlLabel.textContent = 'URL'
    inputUrl.placeholder = 'https://example.com'
    fieldCat.hidden = false; nameLabel.textContent = 'Name'
    submitBtn.textContent = 'Add source'; modalTitle.textContent = 'Add source'
  } else {
    urlLabel.textContent = 'URL'
    inputUrl.placeholder = 'https://example.com/article'
    fieldCat.hidden = true; nameLabel.textContent = 'Title'
    submitBtn.textContent = 'Save link'; modalTitle.textContent = 'Save link'
  }
}

function openModal() {
  document.getElementById('modal-overlay').hidden = false
  document.getElementById('input-name').focus()
  document.getElementById('modal-error').hidden = true
}

function closeModal() {
  document.getElementById('modal-overlay').hidden = true
  document.getElementById('input-name').value = ''
  document.getElementById('input-url').value = ''
  document.getElementById('input-category').value = ''
  document.getElementById('modal-error').hidden = true
  document.getElementById('modal-submit').disabled = false
  updateModalFields()
}

document.getElementById('modal-submit').addEventListener('click', async () => {
  const name     = document.getElementById('input-name').value.trim()
  const url      = document.getElementById('input-url').value.trim()
  const category = document.getElementById('input-category').value.trim()
  const submitBtn = document.getElementById('modal-submit')

  document.getElementById('modal-error').hidden = true
  if (!url) { showModalError('Please enter a URL.'); return }

  submitBtn.disabled = true
  submitBtn.textContent = selectedType === 'link' ? 'Saving…' : 'Adding…'

  if (selectedType === 'link') {
    const item = await window.hub.addLink({ title: name || url, url })
    state.items.unshift(item)
    closeModal()
    renderCategoryPills()
    applyFilters()
    openReader(item)
    return
  }

  if (!name) { showModalError('Please enter a name.'); submitBtn.disabled = false; return }

  // Validate RSS feed URL before adding
  if (selectedType === 'rss') {
    submitBtn.textContent = 'Validating…'
    const validation = await window.hub.validateFeed(url)
    if (!validation.ok) {
      showModalError(`Cannot read feed: ${validation.error}`)
      submitBtn.disabled = false
      updateModalFields()
      return
    }
  }

  const source = await window.hub.addSource({
    name, url, feedUrl: '', type: selectedType, category: category || 'General'
  })
  state.sources.push(source)
  closeModal()
  renderSidebar(); renderCategoryPills()
  await doRefresh()
  updateMainHeader()
})

function showModalError(msg) {
  const el = document.getElementById('modal-error')
  el.textContent = msg; el.hidden = false
  document.getElementById('modal-submit').disabled = false
  updateModalFields()
}

// ── Toast ─────────────────────────────────────────────────────────
let toastTimer   = null
let undoCallback = null
let commitCallback = null

function showToast(msg, type = 'error') {
  const toast    = document.getElementById('toast')
  const undoBtn  = document.getElementById('toast-undo')
  document.getElementById('toast-msg').textContent = msg
  toast.className = 'toast ' + type
  undoBtn.hidden = true
  undoCallback   = null; commitCallback = null
  toast.hidden   = false
  clearTimeout(toastTimer)
  toastTimer = setTimeout(() => { toast.hidden = true }, 8000)
}

function showUndoToast(msg, onUndo, onCommit) {
  const toast   = document.getElementById('toast')
  const undoBtn = document.getElementById('toast-undo')
  document.getElementById('toast-msg').textContent = msg
  toast.className = 'toast info'
  undoBtn.hidden = false
  undoCallback   = onUndo
  commitCallback = onCommit
  toast.hidden   = false
  clearTimeout(toastTimer)
  toastTimer = setTimeout(async () => {
    toast.hidden = true
    undoBtn.hidden = true
    if (commitCallback) await commitCallback()
    undoCallback = null; commitCallback = null
  }, 5000)
}

document.getElementById('toast-undo').addEventListener('click', () => {
  clearTimeout(toastTimer)
  document.getElementById('toast').hidden = true
  document.getElementById('toast-undo').hidden = true
  if (undoCallback) undoCallback()
  undoCallback = null; commitCallback = null
})

document.getElementById('toast-close').addEventListener('click', async () => {
  clearTimeout(toastTimer)
  document.getElementById('toast').hidden = true
  document.getElementById('toast-undo').hidden = true
  if (commitCallback) await commitCallback()
  undoCallback = null; commitCallback = null
})

// ── Command palette ───────────────────────────────────────────────
let cmdActiveIdx = -1

function buildCmdItems(query) {
  const q = query.toLowerCase()
  const items = []

  // Sources
  state.sources.filter(s => !s._pendingDelete).forEach(s => {
    if (!q || s.name.toLowerCase().includes(q)) {
      items.push({
        type: 'source', label: s.name,
        icon: s.type === 'youtube' ? '▶' : '◉',
        meta: s.category || 'General',
        action: () => {
          activeFilter = s.id; activeCategory = 'all'
          closeCmdPalette(); updateMainHeader(); applyFilters()
          document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'))
          document.querySelector(`[data-filter="${s.id}"]`)?.classList.add('active')
        }
      })
    }
  })

  // Categories
  const cats = ['All', 'YouTube', 'RSS', 'Links', 'Saved', 'Later',
    ...[...new Set(state.sources.map(s => s.category || 'General'))]]
  const catMap = { 'All': 'all', 'YouTube': 'youtube', 'RSS': 'rss',
    'Links': 'manual', 'Saved': 'saved', 'Later': 'later' }
  cats.forEach(c => {
    if (!q || c.toLowerCase().includes(q)) {
      items.push({
        type: 'category', label: c, icon: '◈', meta: 'category',
        action: () => {
          activeFilter = 'all'; activeCategory = catMap[c] || c
          closeCmdPalette(); applyFilters()
        }
      })
    }
  })

  // Item titles (when query present)
  if (q) {
    state.items
      .filter(i => i.title && i.title.toLowerCase().includes(q))
      .slice(0, 8)
      .forEach(item => items.push({
        type: 'item', label: item.title,
        icon: item.sourceType === 'youtube' ? '▶' : '◉',
        meta: item.sourceName,
        action: () => { closeCmdPalette(); openReader(item) }
      }))
  }

  // Actions
  const actions = [
    { label: 'Refresh all',       icon: '↻', meta: 'action', action: () => { closeCmdPalette(); doRefresh() } },
    { label: 'Add source',        icon: '+', meta: 'action', action: () => { closeCmdPalette(); openModal() } },
    { label: 'Toggle view mode',  icon: '⊡', meta: 'action', action: () => { closeCmdPalette(); document.getElementById('view-toggle').click() } },
    { label: 'Toggle hide read',  icon: '◌', meta: 'action', action: () => { closeCmdPalette(); document.getElementById('hide-read-btn').click() } },
    { label: 'Focus mode',        icon: '⤢', meta: 'action', action: () => { closeCmdPalette(); document.body.classList.toggle('focus') } },
  ]
  actions.forEach(a => {
    if (!q || a.label.toLowerCase().includes(q)) items.push(a)
  })

  return items
}

function renderCmdList(query) {
  const list  = document.getElementById('cmd-list')
  cmdActiveIdx = -1
  list.innerHTML = ''
  const items = buildCmdItems(query)
  if (!items.length) {
    list.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-3);font-size:12px">No results</div>'
    return
  }
  items.forEach((item, i) => {
    const el = document.createElement('div')
    el.className = 'cmd-item'
    el.innerHTML = `<span class="cmd-item-icon">${item.icon}</span>
      <span class="cmd-item-label">${item.label}</span>
      <span class="cmd-item-type">${item.meta}</span>`
    el.addEventListener('click', item.action)
    el.dataset.idx = i
    list.appendChild(el)
  })
}

function openCmdPalette() {
  const overlay = document.getElementById('cmd-palette-overlay')
  overlay.hidden = false
  const input = document.getElementById('cmd-input')
  input.value = ''
  renderCmdList('')
  input.focus()

  input.oninput = () => renderCmdList(input.value.trim())
  input.onkeydown = e => {
    const items = document.querySelectorAll('.cmd-item')
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      cmdActiveIdx = Math.min(cmdActiveIdx + 1, items.length - 1)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      cmdActiveIdx = Math.max(cmdActiveIdx - 1, 0)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const active = items[cmdActiveIdx] || items[0]
      if (active) active.click()
      return
    } else if (e.key === 'Escape') {
      closeCmdPalette(); return
    }
    items.forEach((el, i) => el.classList.toggle('active', i === cmdActiveIdx))
    items[cmdActiveIdx]?.scrollIntoView({ block: 'nearest' })
  }
}

function closeCmdPalette() {
  document.getElementById('cmd-palette-overlay').hidden = true
}

document.getElementById('cmd-palette-overlay').addEventListener('click', e => {
  if (e.target === document.getElementById('cmd-palette-overlay')) closeCmdPalette()
})

// ── Status bar ────────────────────────────────────────────────────
function updateStatusBar() {
  document.getElementById('status-items').textContent = state.items.length + ' items'
  const onEl = document.getElementById('status-online')
  onEl.className = navigator.onLine ? 'online' : 'offline'
  onEl.title     = navigator.onLine ? 'Online' : 'Offline'
}
function setLastRefresh() {
  document.getElementById('status-refresh').textContent =
    'Refreshed ' + new Date().toLocaleTimeString('en', { hour: '2-digit', minute: '2-digit' })
}
window.addEventListener('online',  updateStatusBar)
window.addEventListener('offline', updateStatusBar)

// ── Settings panel ────────────────────────────────────────────────
function applyAccentColor(color) {
  if (!color) return
  document.documentElement.style.setProperty('--accent', color)
  const r = parseInt(color.slice(1, 3), 16)
  const g = parseInt(color.slice(3, 5), 16)
  const b = parseInt(color.slice(5, 7), 16)
  document.documentElement.style.setProperty('--accent-dim', `rgba(${r},${g},${b},0.12)`)
}
function applyFontSize(fs) {
  document.documentElement.style.setProperty('--reader-fs', fs + 'px')
}

function openSettings() {
  document.getElementById('settings-panel').hidden = false
  const s = document.getElementById('setting-accent')
  const style = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()
  if (style) s.value = style.startsWith('#') ? style : '#5b9cf6'
  const fs = parseInt(getComputedStyle(document.documentElement).getPropertyValue('--reader-fs')) || 16
  document.getElementById('setting-fontsize').value = fs
  document.getElementById('setting-fontsize-val').textContent = fs
}
function closeSettings() { document.getElementById('settings-panel').hidden = true }

document.getElementById('settings-btn').addEventListener('click', () => {
  document.getElementById('settings-panel').hidden ? openSettings() : closeSettings()
})
document.getElementById('settings-close').addEventListener('click', closeSettings)

document.getElementById('setting-accent').addEventListener('input', async e => {
  applyAccentColor(e.target.value)
  await window.hub.saveSettings({ accentColor: e.target.value })
})

document.getElementById('setting-fontsize').addEventListener('input', async e => {
  const fs = parseInt(e.target.value)
  document.getElementById('setting-fontsize-val').textContent = fs
  applyFontSize(fs)
  await window.hub.saveSettings({ fontSize: fs })
})

// ── Daily digest ──────────────────────────────────────────────────
function checkDigest() {
  const lastSeen = localStorage.getItem('hub-last-seen')
  if (!lastSeen) { localStorage.setItem('hub-last-seen', new Date().toISOString()); return }
  const since = new Date(lastSeen)
  const newItems      = state.items.filter(i => i.date && new Date(i.date) > since)
  const unreadSaved   = state.items.filter(i => i.saved && !i.read)
  const weekAgo       = Date.now() - 7 * 864e5
  const recentHl      = highlights.filter(h => new Date(h.createdAt) > weekAgo)
  if (!newItems.length && !unreadSaved.length && !recentHl.length) return
  const parts = []
  if (newItems.length)    parts.push(`${newItems.length} new item${newItems.length > 1 ? 's' : ''}`)
  if (unreadSaved.length) parts.push(`${unreadSaved.length} saved unread`)
  if (recentHl.length)    parts.push(`${recentHl.length} highlight${recentHl.length > 1 ? 's' : ''} this week`)
  document.getElementById('digest-text').textContent = parts.join(' · ')
  document.getElementById('digest-banner').hidden = false
  localStorage.setItem('hub-last-seen', new Date().toISOString())
}

document.getElementById('digest-dismiss').addEventListener('click', () => {
  document.getElementById('digest-banner').hidden = true
})

// ── Graph view ────────────────────────────────────────────────────
class GraphView {
  constructor(canvas) {
    this.canvas = canvas
    this.ctx    = canvas.getContext('2d')
    this.nodes  = []; this.edges  = []
    this.pan    = { x: 0, y: 0 }; this.scale = 1
    this.dragging = null; this.panning = false; this._panStart = null
    this.hovered  = null
    this.raf    = null
    this._bind()
  }

  setData(items, links) {
    // Include saved items + any item referenced in links
    const linkedIds = new Set()
    for (const l of links) { linkedIds.add(l.fromItemId); linkedIds.add(l.toItemId) }
    const relevant = items.filter(i => i.saved || linkedIds.has(i.id))
    const idxMap = new Map(relevant.map((item, i) => [item.id, i]))
    const cx = this.canvas.width / 2, cy = this.canvas.height / 2

    // Count links per node for radius
    const linkCount = new Map()
    for (const l of links) {
      linkCount.set(l.fromItemId, (linkCount.get(l.fromItemId) || 0) + 1)
      linkCount.set(l.toItemId, (linkCount.get(l.toItemId) || 0) + 1)
    }

    // Group by category for initial positioning
    const catGroups = new Map()
    relevant.forEach(item => {
      const src = state.sources.find(s => s.id === item.sourceId)
      const cat = src?.category || 'General'
      if (!catGroups.has(cat)) catGroups.set(cat, [])
      catGroups.get(cat).push(item)
    })
    const catList = [...catGroups.keys()]
    const catAngle = (2 * Math.PI) / Math.max(catList.length, 1)
    const baseR = Math.min(cx, cy) * 0.5

    this.nodes = relevant.map((item, i) => {
      const src = state.sources.find(s => s.id === item.sourceId)
      const cat = src?.category || 'General'
      const catIdx = catList.indexOf(cat)
      const group = catGroups.get(cat)
      const posInGroup = group.indexOf(item)
      const groupSpread = Math.min(group.length * 18, 120)
      const catCx = cx + baseR * Math.cos(catIdx * catAngle)
      const catCy = cy + baseR * Math.sin(catIdx * catAngle)
      const lc = linkCount.get(item.id) || 0
      const radius = Math.max(4, Math.min(14, 4 + lc * 2.5))
      return {
        id: item.id, item, label: item.title.slice(0, 28),
        color: hashColor(item.sourceName || ''),
        x: catCx + (Math.random() - 0.5) * groupSpread,
        y: catCy + (Math.random() - 0.5) * groupSpread,
        vx: 0, vy: 0, fixed: false, radius,
        linkCount: lc, opacity: lc > 0 ? 1 : 0.45,
      }
    })

    this.edges = []
    for (const l of links) {
      const ai = idxMap.get(l.fromItemId)
      const bi = idxMap.get(l.toItemId)
      if (ai !== undefined && bi !== undefined) {
        this.edges.push({ a: ai, b: bi, w: 1.2, color: 'var(--border)' })
      }
    }

    // Also add same-source edges (lighter)
    for (let i = 0; i < this.nodes.length; i++) {
      for (let j = i + 1; j < this.nodes.length; j++) {
        const a = this.nodes[i].item, b = this.nodes[j].item
        if (a.sourceId === b.sourceId && !this.edges.find(e => (e.a === i && e.b === j) || (e.a === j && e.b === i))) {
          // Don't add — keep graph clean, only wikilink edges
        }
      }
    }

    for (let k = 0; k < 300; k++) this._tick()
  }

  _tick() {
    const cx = this.canvas.width / 2, cy = this.canvas.height / 2
    for (let i = 0; i < this.nodes.length; i++) {
      for (let j = i + 1; j < this.nodes.length; j++) {
        const a = this.nodes[i], b = this.nodes[j]
        const dx = b.x - a.x, dy = b.y - a.y
        const d2 = Math.max(dx * dx + dy * dy, 1), d = Math.sqrt(d2)
        const f  = 3500 / d2
        a.vx -= f * dx / d; a.vy -= f * dy / d
        b.vx += f * dx / d; b.vy += f * dy / d
      }
    }
    for (const e of this.edges) {
      const a = this.nodes[e.a], b = this.nodes[e.b]
      const dx = b.x - a.x, dy = b.y - a.y
      const d  = Math.sqrt(dx * dx + dy * dy) || 1
      const f  = (d - 130) * 0.004
      a.vx += f * dx / d; a.vy += f * dy / d
      b.vx -= f * dx / d; b.vy -= f * dy / d
    }
    for (const n of this.nodes) {
      if (n.fixed) { n.vx = 0; n.vy = 0; continue }
      n.vx += (cx - n.x) * 0.0006; n.vy += (cy - n.y) * 0.0006
      n.vx *= 0.82; n.vy *= 0.82
      n.x  += n.vx; n.y  += n.vy
    }
  }

  _draw() {
    const { ctx, canvas } = this
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    ctx.save()
    ctx.translate(this.pan.x, this.pan.y)
    ctx.scale(this.scale, this.scale)

    const hId = this.hovered?.id
    const neighborIds = new Set()
    if (hId) {
      this.edges.forEach(e => {
        if (this.nodes[e.a].id === hId) neighborIds.add(this.nodes[e.b].id)
        if (this.nodes[e.b].id === hId) neighborIds.add(this.nodes[e.a].id)
      })
    }

    // Edges
    const borderColor = getComputedStyle(document.documentElement).getPropertyValue('--border').trim() || '#2a2a2a'
    for (const e of this.edges) {
      const a = this.nodes[e.a], b = this.nodes[e.b]
      const highlight = hId && (a.id === hId || b.id === hId)
      const dim = hId && !highlight
      ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y)
      ctx.strokeStyle = highlight ? 'rgba(91,156,246,0.6)' : dim ? 'rgba(42,42,42,0.15)' : borderColor
      ctx.lineWidth = highlight ? 2 : e.w
      ctx.stroke()
    }

    // Nodes
    for (const n of this.nodes) {
      const isHovered = n.id === hId
      const isNeighbor = neighborIds.has(n.id)
      const dim = hId && !isHovered && !isNeighbor
      const alpha = dim ? 0.15 : n.opacity
      ctx.globalAlpha = alpha
      ctx.beginPath(); ctx.arc(n.x, n.y, n.radius, 0, Math.PI * 2)
      ctx.fillStyle = n.color; ctx.fill()
      if (isHovered) {
        ctx.strokeStyle = 'rgba(255,255,255,0.6)'; ctx.lineWidth = 2; ctx.stroke()
      }
      ctx.font = `${isHovered ? 11 : 9}px ui-monospace, monospace`
      ctx.fillStyle = dim ? 'rgba(180,180,180,0.15)' : 'rgba(180,180,180,0.65)'
      ctx.fillText(n.label, n.x + n.radius + 4, n.y + 3)
      ctx.globalAlpha = 1
    }
    ctx.restore()
  }

  start() {
    const loop = () => { this._tick(); this._draw(); this.raf = requestAnimationFrame(loop) }
    this.raf = requestAnimationFrame(loop)
  }
  stop() { if (this.raf) { cancelAnimationFrame(this.raf); this.raf = null } }

  _wp(sx, sy) {
    return { x: (sx - this.pan.x) / this.scale, y: (sy - this.pan.y) / this.scale }
  }
  _hit(wx, wy) {
    return this.nodes.find(n => {
      const dx = n.x - wx, dy = n.y - wy
      return Math.sqrt(dx * dx + dy * dy) <= n.radius + 5
    }) || null
  }

  _bind() {
    const c = this.canvas
    c.addEventListener('mousedown', e => {
      const wp = this._wp(e.offsetX, e.offsetY)
      const n  = this._hit(wp.x, wp.y)
      if (n) { this.dragging = n; n.fixed = true }
      else   { this.panning = true; this._panStart = { x: e.offsetX - this.pan.x, y: e.offsetY - this.pan.y } }
    })
    c.addEventListener('mouseup', () => {
      if (this.dragging) { this.dragging.fixed = false; this.dragging = null }
      this.panning = false
    })
    c.addEventListener('wheel', e => {
      e.preventDefault()
      this.scale = Math.max(0.2, Math.min(4, this.scale * (e.deltaY > 0 ? 0.92 : 1.08)))
    }, { passive: false })
    c.addEventListener('mousemove', e => {
      if (!this.dragging && !this.panning) {
        const wp = this._wp(e.offsetX, e.offsetY)
        const n = this._hit(wp.x, wp.y)
        this.hovered = n
        c.style.cursor = n ? 'pointer' : 'default'
      }
      if (this.dragging) {
        const wp = this._wp(e.offsetX, e.offsetY)
        this.dragging.x = wp.x; this.dragging.y = wp.y
        this.dragging.vx = 0; this.dragging.vy = 0
      } else if (this.panning && this._panStart) {
        this.pan.x = e.offsetX - this._panStart.x
        this.pan.y = e.offsetY - this._panStart.y
      }
    })
    c.addEventListener('click', e => {
      const wp = this._wp(e.offsetX, e.offsetY)
      const n  = this._hit(wp.x, wp.y)
      if (n) { closeGraphView(); openReader(n.item) }
    })
  }
}

async function openGraphView() {
  const overlay = document.getElementById('graph-overlay')
  overlay.hidden = false
  const canvas = document.getElementById('graph-canvas')
  canvas.width  = canvas.offsetWidth
  canvas.height = canvas.offsetHeight

  // Populate filter dropdowns
  const catSelect = document.getElementById('graph-filter-cat')
  const tagSelect = document.getElementById('graph-filter-tag')
  catSelect.innerHTML = '<option value="all">All categories</option>'
  tagSelect.innerHTML = '<option value="all">All tags</option>'
  const cats = [...new Set(state.sources.map(s => s.category || 'General'))]
  cats.forEach(c => { catSelect.innerHTML += `<option value="${c}">${c}</option>` })
  const tags = [...new Set(state.items.flatMap(i => i.tags || []))]
  tags.forEach(t => { tagSelect.innerHTML += `<option value="${t}">#${t}</option>` })
  catSelect.value = graphFilterCat
  tagSelect.value = graphFilterTag

  await refreshGraph()
}

async function refreshGraph() {
  const canvas = document.getElementById('graph-canvas')
  const links = await window.hub.getAllLinks()

  // Filter items by category/tag
  let filteredItems = state.items
  if (graphFilterCat !== 'all') {
    const catSourceIds = new Set(state.sources.filter(s => (s.category || 'General') === graphFilterCat).map(s => s.id))
    filteredItems = filteredItems.filter(i => catSourceIds.has(i.sourceId))
  }
  if (graphFilterTag !== 'all') {
    filteredItems = filteredItems.filter(i => (i.tags || []).includes(graphFilterTag))
  }
  const filteredIds = new Set(filteredItems.map(i => i.id))
  const filteredLinks = links.filter(l => filteredIds.has(l.fromItemId) || filteredIds.has(l.toItemId))

  if (!graphInstance) graphInstance = new GraphView(canvas)
  else { graphInstance.canvas = canvas; graphInstance.ctx = canvas.getContext('2d'); graphInstance.hovered = null }
  graphInstance.setData(filteredItems, filteredLinks)
  graphInstance.start()
}

document.getElementById('graph-filter-cat')?.addEventListener('change', e => {
  graphFilterCat = e.target.value
  refreshGraph()
})
document.getElementById('graph-filter-tag')?.addEventListener('change', e => {
  graphFilterTag = e.target.value
  refreshGraph()
})

function closeGraphView() {
  document.getElementById('graph-overlay').hidden = true
  graphInstance?.stop()
}

document.getElementById('graph-close').addEventListener('click', closeGraphView)

// ── Keyboard shortcuts ────────────────────────────────────────────
document.addEventListener('keydown', e => {
  const search = document.getElementById('search-input')
  const modal  = document.getElementById('modal-overlay')

  if ((e.metaKey || e.ctrlKey) && e.key === '1') {
    e.preventDefault()
    activeFilter = 'all'; activeCategory = 'all'
    document.querySelectorAll('.nav-item').forEach(n => n.classList.remove('active'))
    document.querySelector('[data-filter="all"]')?.classList.add('active')
    updateMainHeader(); applyFilters(); return
  }
  if ((e.metaKey || e.ctrlKey) && e.key === '2') {
    e.preventDefault()
    activeFilter = 'all'; activeCategory = 'saved'
    renderCategoryPills(); applyFilters(); return
  }
  if ((e.metaKey || e.ctrlKey) && (e.key === '3' || e.key === 'g')) {
    e.preventDefault()
    if (!document.getElementById('graph-overlay').hidden) closeGraphView()
    else openGraphView()
    return
  }
  if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
    e.preventDefault()
    if (!document.getElementById('cmd-palette-overlay').hidden) closeCmdPalette()
    else openCmdPalette()
    return
  }
  if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
    e.preventDefault(); search.focus(); return
  }
  if ((e.metaKey || e.ctrlKey) && e.key === 'n') {
    e.preventDefault()
    openStandaloneEditor(null)
    return
  }
  if (e.key === 'Escape') {
    if (!document.getElementById('standalone-overlay').hidden) { closeStandaloneEditor(); return }
    if (!document.getElementById('quick-note-popover').hidden) { document.getElementById('quick-note-popover').hidden = true; quickNoteItem = null; return }
    if (!document.getElementById('graph-overlay').hidden) { closeGraphView(); return }
    if (!document.getElementById('cmd-palette-overlay').hidden) { closeCmdPalette(); return }
    if (!modal.hidden) { closeModal(); return }
    if (document.activeElement === search) {
      search.value = ''; searchQuery = ''; applyFilters(); search.blur(); return
    }
    if (readerItem) { closeReader(); return }
    if (!document.getElementById('stats-panel').hidden) {
      closeStats(); return
    }
    if (!document.getElementById('notes-panel').hidden) {
      closeNotes(); return
    }
    if (!document.getElementById('settings-panel').hidden) {
      closeSettings(); return
    }
  }
  if (e.key === 'f' && !modal.hidden === false && document.activeElement.tagName !== 'INPUT' && document.activeElement.tagName !== 'TEXTAREA') {
    document.body.classList.toggle('focus')
  }
  if ((e.metaKey || e.ctrlKey) && e.key === 'r') {
    e.preventDefault(); doRefresh()
  }
})

// ── Boot ──────────────────────────────────────────────────────────
init()
