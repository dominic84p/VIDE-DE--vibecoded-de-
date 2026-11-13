// Contributors loader
class ContributorsLoader {
    constructor() {
        this.list = document.getElementById('contributors-list');
        this.stats = {
            contributors: document.getElementById('stat-contributors'),
            commits: document.getElementById('stat-commits'),
            files: document.getElementById('stat-files'),
            lines: document.getElementById('stat-lines')
        };
        
        this.init();
    }
    
    async init() {
        try {
            // Fetch contributors from GitHub API
            let page = 1;
            let allContributors = [];
            
            while (true) {
                const response = await fetch(`https://api.github.com/repos/dominic84p/vide-de/contributors?per_page=100&page=${page}`);
                if (!response.ok) break;
                
                const data = await response.json();
                if (data.length === 0) break;
                
                allContributors = allContributors.concat(data);
                page++;
            }
            
            // Fetch commit stats for each contributor
            const enriched = await Promise.all(
                allContributors.map(async (c) => {
                    try {
                        const commitsResponse = await fetch(`https://api.github.com/repos/dominic84p/vide-de/commits?author=${c.login}&per_page=1`);
                        const commits = await commitsResponse.json();
                        const commitCount = commits.length > 0 ? 
                            parseInt(commitsResponse.headers.get('Link')?.match(/page=(\d+)>; rel="last"/)?.[1] || '1') : 0;
                        
                        // Get stats from first commit to estimate
                        let totalFiles = 0, totalLines = 0;
                        if (commitCount > 0) {
                            const statsResponse = await fetch(`https://api.github.com/repos/dominic84p/vide-de/commits?author=${c.login}&per_page=100`);
                            const recentCommits = await statsResponse.json();
                            for (const commit of recentCommits.slice(0, 10)) {
                                try {
                                    const detailResponse = await fetch(commit.url);
                                    const detail = await detailResponse.json();
                                    totalFiles += detail.files?.length || 0;
                                    totalLines += detail.stats?.total || 0;
                                } catch {}
                            }
                        }
                        
                        return {
                            ...c,
                            commitCount,
                            totalFiles,
                            totalLines
                        };
                    } catch {
                        return { ...c, commitCount: 0, totalFiles: 0, totalLines: 0 };
                    }
                })
            );
            
            this.render(enriched);
            this.updateStats(enriched);
        } catch (e) {
            this.list.innerHTML = '<div class="contributors-loading">Failed to load contributors</div>';
            console.error(e);
        }
    }
    
    updateStats(contributors) {
        const totalCommits = contributors.reduce((sum, c) => sum + c.commitCount, 0);
        const totalFiles = contributors.reduce((sum, c) => sum + c.totalFiles, 0);
        const totalLines = contributors.reduce((sum, c) => sum + c.totalLines, 0);
        
        this.stats.contributors.textContent = contributors.length;
        this.stats.commits.textContent = totalCommits.toLocaleString();
        this.stats.files.textContent = totalFiles.toLocaleString();
        this.stats.lines.textContent = totalLines.toLocaleString();
    }
    
    render(contributors) {
        // Sort by commit count
        contributors.sort((a, b) => b.commitCount - a.commitCount);
        
        this.list.innerHTML = contributors.map(c => `
            <article class="contributor-card">
                <div class="contributor-header">
                    <img class="contributor-avatar" 
                         src="${c.avatar_url}&s=112" 
                         alt="${c.login}"
                         loading="lazy">
                    <div class="contributor-info">
                        <h4>${c.login}</h4>
                        <span class="model">AI Agent</span>
                    </div>
                </div>
                <div class="contributor-stats">
                    <div class="contributor-stat">
                        <span class="contributor-stat-value">${c.commitCount.toLocaleString()}</span>
                        <span class="contributor-stat-label">Commits</span>
                    </div>
                    <div class="contributor-stat">
                        <span class="contributor-stat-value">${c.totalFiles.toLocaleString()}</span>
                        <span class="contributor-stat-label">Files</span>
                    </div>
                    <div class="contributor-stat">
                        <span class="contributor-stat-value">${c.totalLines.toLocaleString()}</span>
                        <span class="contributor-stat-label">Lines</span>
                    </div>
                </div>
                <div class="contributor-links">
                    <a href="${c.html_url}" target="_blank" rel="noopener">
                        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
                            <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/>
                        </svg>
                        Profile
                    </a>
                    <a href="https://github.com/dominic84p/vide-de/commits?author=${c.login}" target="_blank" rel="noopener">
                        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor">
                            <path d="M2.5 0A2.5 2.5 0 000 2.5v11A2.5 2.5 0 002.5 16h11a2.5 2.5 0 002.5-2.5v-11A2.5 2.5 0 0013.5 0h-11zm11.5 1.5a1 1 0 011 1v11a1 1 0 01-1 1h-11a1 1 0 01-1-1v-11a1 1 0 011-1h11zM11 11a1 1 0 00-1-1H6a1 1 0 00-1 1v1a1 1 0 001 1h4a1 1 0 001-1v-1zm0-3a1 1 0 00-1-1H6a1 1 0 00-1 1v1a1 1 0 001 1h4a1 1 0 001-1V8zm-4 4a1 1 0 011-1h2a1 1 0 011 1v1a1 1 0 01-1 1H8a1 1 0 01-1-1v-1zm0-3a1 1 0 011-1h2a1 1 0 011 1v1a1 1 0 01-1 1H8a1 1 0 01-1-1V8z"/>
                        </svg>
                        Commits
                    </a>
                </div>
            </article>
        `).join('');
    }
}

document.addEventListener('DOMContentLoaded', () => {
    new ContributorsLoader();
});