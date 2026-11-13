// Git browser client-side logic
class GitBrowser {
    constructor() {
        this.tree = document.getElementById('file-tree');
        this.view = document.getElementById('file-view');
        this.search = document.getElementById('file-search');
        this.files = [];
        this.currentFile = null;
        
        this.init();
    }
    
    async init() {
        try {
            // Fetch file list from GitHub API
            const response = await fetch('https://api.github.com/repos/dominic84p/vide-de/git/trees/main?recursive=1');
            const data = await response.json();
            
            this.files = data.tree
                .filter(item => item.type === 'blob')
                .filter(item => !item.path.includes('node_modules') && !item.path.includes('target') && !item.path.includes('.git'))
                .map(item => item.path)
                .sort();
            
            this.renderTree();
            this.setupSearch();
        } catch (e) {
            this.tree.innerHTML = '<div class="tree-loading">Failed to load repository</div>';
            console.error(e);
        }
    }
    
    renderTree(filter = '') {
        const filtered = this.files.filter(f => f.toLowerCase().includes(filter.toLowerCase()));
        
        // Build tree structure
        const root = {};
        for (const path of filtered) {
            const parts = path.split('/');
            let node = root;
            for (let i = 0; i < parts.length; i++) {
                const part = parts[i];
                if (!node[part]) {
                    node[part] = { type: i === parts.length - 1 ? 'file' : 'folder', children: {} };
                }
                node = node[part].children;
            }
        }
        
        this.tree.innerHTML = this.renderNode(root, '');
    }
    
    renderNode(node, prefix) {
        let html = '';
        const entries = Object.entries(node).sort(([a], [b]) => {
            const aIsFolder = node[a].type === 'folder';
            const bIsFolder = node[b].type === 'folder';
            if (aIsFolder !== bIsFolder) return aIsFolder ? -1 : 1;
            return a.localeCompare(b);
        });
        
        for (const [name, { type, children }] of entries) {
            const fullPath = prefix + name;
            if (type === 'folder') {
                html += `<details class="tree-folder" open>
                    <summary><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg> ${name}</summary>
                    <div>${this.renderNode(children, fullPath + '/')}</div>
                </details>`;
            } else {
                html += `<a class="tree-file" href="#" data-path="${fullPath}">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>
                    ${name}
                </a>`;
            }
        }
        return html;
    }
    
    setupSearch() {
        let debounce;
        this.search.addEventListener('input', (e) => {
            clearTimeout(debounce);
            debounce = setTimeout(() => {
                this.renderTree(e.target.value);
            }, 150);
        });
    }
    
    async loadFile(path) {
        // Highlight selected
        this.tree.querySelectorAll('.tree-file').forEach(el => {
            el.classList.toggle('selected', el.dataset.path === path);
        });
        
        this.view.innerHTML = '<div class="view-placeholder">Loading...</div>';
        
        try {
            const response = await fetch(`https://raw.githubusercontent.com/dominic84p/vide-de/main/${path}`);
            const content = await response.text();
            this.renderFile(path, content);
        } catch (e) {
            this.view.innerHTML = `<div class="view-placeholder">Failed to load file: ${e.message}</div>`;
        }
    }
    
    renderFile(path, content) {
        const ext = path.split('.').pop().toLowerCase();
        const lines = content.split('\n');
        
        // Basic syntax highlighting
        const highlighted = lines.map((line, i) => {
            let html = this.highlightLine(line, ext);
            return `<div class="line"><span class="line-number">${i + 1}</span><span class="line-content">${html}</span></div>`;
        }).join('');
        
        this.view.innerHTML = `
            <div class="file-header">
                <span class="file-path">${path}</span>
                <div class="file-actions">
                    <button onclick="navigator.clipboard.writeText(\`${content.replace(/`/g, '\\`')}\`)">Copy</button>
                    <a href="https://github.com/dominic84p/vide-de/blob/main/${path}" target="_blank" rel="noopener" style="text-decoration:none">
                        <button>GitHub</button>
                    </a>
                </div>
            </div>
            <div class="file-content"><pre><code>${highlighted}</code></pre></div>
        `;
    }
    
    highlightLine(line, ext) {
        // Very basic highlighting - replace with proper highlighter if needed
        return line
            .replace(/&/g, '&')
            .replace(/</g, '<')
            .replace(/>/g, '>')
            .replace(/(^\s*)(\/\/.*)$/, '$1<span class="token comment">$2</span>')
            .replace(/\b(fn|let|mut|const|struct|enum|impl|trait|mod|use|pub|async|await|match|if|else|for|while|loop|return|break|continue|type|as|where|dyn|ref|move|static|unsafe|extern|crate|super|self|Self)\b/g, '<span class="token keyword">$1</span>')
            .replace(/\b(String|Vec|Option|Result|Box|Arc|Rc|HashMap|HashSet)\b/g, '<span class="token class-name">$1</span>')
            .replace(/("([^"\\]|\\.)*")/g, '<span class="token string">$1</span>')
            .replace(/(\b\d+\.?\d*\b)/g, '<span class="token number">$1</span>');
    }
}

// Initialize on DOM ready
document.addEventListener('DOMContentLoaded', () => {
    const browser = new GitBrowser();
    
    // Event delegation for file clicks
    document.getElementById('file-tree').addEventListener('click', (e) => {
        const link = e.target.closest('.tree-file');
        if (link) {
            e.preventDefault();
            browser.loadFile(link.dataset.path);
        }
    });
});