// Changelog loader
class ChangelogLoader {
    constructor() {
        this.content = document.getElementById('changelog-content');
        this.select = document.getElementById('version-select');
        this.commits = [];
        this.versions = new Map();
        
        this.init();
    }
    
    async init() {
        try {
            // Fetch commits from GitHub API
            let page = 1;
            let allCommits = [];
            
            while (page <= 5) { // Limit to 5 pages (500 commits)
                const response = await fetch(`https://api.github.com/repos/dominic84p/vide-de/commits?per_page=100&page=${page}`);
                if (!response.ok) break;
                
                const data = await response.json();
                if (data.length === 0) break;
                
                allCommits = allCommits.concat(data);
                page++;
            }
            
            this.commits = allCommits;
            this.processCommits();
            this.renderSelect();
            this.render('all');
        } catch (e) {
            this.content.innerHTML = '<div class="changelog-loading">Failed to load changelog</div>';
            console.error(e);
        }
    }
    
    processCommits() {
        for (const commit of this.commits) {
            const message = commit.commit.message.split('\n')[0];
            const date = new Date(commit.commit.author.date);
            const version = this.extractVersion(message) || 'Unreleased';
            
            if (!this.versions.has(version)) {
                this.versions.set(version, {
                    tag: version,
                    date: date,
                    commits: []
                });
            }
            
            this.versions.get(version).commits.push({
                hash: commit.sha.substring(0, 7),
                message: message,
                author: commit.commit.author.name,
                date: date,
                url: commit.html_url
            });
        }
        
        // Sort versions by date (newest first)
        this.versions = new Map([...this.versions.entries()].sort((a, b) => b[1].date - a[1].date));
    }
    
    extractVersion(message) {
        // Match conventional commit version tags like "v0.1.0" or "Release v0.1.0"
        const match = message.match(/(?:v?\d+\.\d+\.\d+|Release v?\d+\.\d+\.\d+)/i);
        if (match) return match[0].replace('Release ', '');
        
        // Fallback: group by month
        return null;
    }
    
    renderSelect() {
        const options = ['<option value="all">All versions</option>'];
        for (const [version, data] of this.versions) {
            const dateStr = data.date.toLocaleDateString('en-US', { year: 'numeric', month: 'short' });
            options.push(`<option value="${version}">${version} (${dateStr}) - ${data.commits.length} commits</option>`);
        }
        this.select.innerHTML = options.join('');
        
        this.select.addEventListener('change', (e) => this.render(e.target.value));
    }
    
    render(filter) {
        let html = '';
        
        for (const [version, data] of this.versions) {
            if (filter !== 'all' && version !== filter) continue;
            
            const dateStr = data.date.toLocaleDateString('en-US', { 
                year: 'numeric', month: 'long', day: 'numeric' 
            });
            
            html += `
                <div class="changelog-version">
                    <div class="changelog-version-header">
                        <span class="changelog-version-tag">${version}</span>
                        <span class="changelog-version-date">${dateStr}</span>
                    </div>
                    <div class="changelog-commits">
            `;
            
            for (const commit of data.commits) {
                const isBreaking = commit.message.toLowerCase().includes('breaking') || commit.message.includes('!');
                const msgHtml = commit.message
                    .replace(/&/g, '&')
                    .replace(/</g, '<')
                    .replace(/>/g, '>');
                
                html += `
                    <div class="changelog-commit">
                        <a class="changelog-commit-hash" href="${commit.url}" target="_blank" rel="noopener">${commit.hash}</a>
                        <div class="changelog-commit-message">
                            ${isBreaking ? '<span class="breaking">BREAKING</span>' : ''}${msgHtml}
                        </div>
                        <span class="changelog-commit-author">${commit.author}</span>
                    </div>
                `;
            }
            
            html += `
                    </div>
                </div>
            `;
        }
        
        if (!html) {
            html = '<div class="changelog-empty">No commits found for this filter</div>';
        }
        
        this.content.innerHTML = html;
    }
}

document.addEventListener('DOMContentLoaded', () => {
    new ChangelogLoader();
});