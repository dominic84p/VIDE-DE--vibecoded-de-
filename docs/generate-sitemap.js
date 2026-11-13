/**
 * Sitemap generator for VIDE-DE docs
 * Run after build: node generate-sitemap.js
 */
import fs from 'fs';
import path from 'path';

const SITE_URL = 'https://dominic84p.github.io';
const BASE_PATH = '/vide-de/';

async function generateSitemap() {
    const distDir = path.join(process.cwd(), 'dist');
    
    if (!fs.existsSync(distDir)) {
        console.error('dist/ directory not found. Run `npm run build` first.');
        process.exit(1);
    }
    
    const urls = new Set();
    const today = new Date().toISOString().split('T')[0];
    
    function walk(dir) {
        const entries = fs.readdirSync(dir, { withFileTypes: true });
        
        for (const entry of entries) {
            const fullPath = path.join(dir, entry.name);
            
            if (entry.isDirectory()) {
                walk(fullPath);
            } else if (entry.name === 'index.html') {
                // Convert file path to URL
                let urlPath = path.relative(distDir, fullPath)
                    .replace(/\\/g, '/')
                    .replace(/\/index\.html$/, '/')
                    .replace(/index\.html$/, '/');
                
                if (urlPath === '') urlPath = '/';
                
                urls.add(SITE_URL + BASE_PATH + urlPath);
            }
        }
    }
    
    walk(distDir);
    
    // Add special pages
    urls.add(SITE_URL + BASE_PATH + 'git-browser/');
    urls.add(SITE_URL + BASE_PATH + 'changelog/');
    urls.add(SITE_URL + BASE_PATH + 'contributors/');
    urls.add(SITE_URL + BASE_PATH + '404.html');
    
    // Generate sitemap XML
    const sortedUrls = [...urls].sort();
    
    let xml = '<?xml version="1.0" encoding="UTF-8"?>\n';
    xml += '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n';
    
    for (const url of sortedUrls) {
        xml += '  <url>\n';
        xml += `    <loc>${url}</loc>\n`;
        xml += `    <lastmod>${today}</lastmod>\n`;
        xml += '    <changefreq>weekly</changefreq>\n';
        xml += '    <priority>0.8</priority>\n';
        xml += '  </url>\n';
    }
    
    xml += '</urlset>\n';
    
    const sitemapPath = path.join(distDir, 'sitemap.xml');
    fs.writeFileSync(sitemapPath, xml);
    
    console.log(`✅ Sitemap generated: ${sitemapPath}`);
    console.log(`   ${sortedUrls.length} URLs`);
}

generateSitemap().catch(console.error);