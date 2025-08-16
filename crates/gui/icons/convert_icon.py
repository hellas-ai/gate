#!/usr/bin/env python3
import subprocess
import sys
import os
import argparse
from PIL import Image
import cairosvg

def convert_svg_to_png(svg_path, png_path, width, height):
    """Convert SVG to PNG using cairosvg"""
    cairosvg.svg2png(url=svg_path, write_to=png_path, 
                     output_width=width, output_height=height)
    print(f"Created {png_path} ({width}x{height})")
    return True

def create_ico_file(png_sizes, ico_path):
    """Create Windows .ico file from multiple PNG sizes"""
    # Load all PNG images
    images = []
    for size, path in png_sizes:
        if os.path.exists(path):
            img = Image.open(path)
            images.append(img)
    
    if images:
        # Save as ICO with multiple sizes
        images[0].save(ico_path, format='ICO', sizes=[(img.width, img.height) for img in images])
        print(f"Created {ico_path} with {len(images)} sizes")
        return True
    return False

def create_icns_file(svg_file, icns_path):
    """Create macOS .icns file"""
    # Create iconset directory structure
    iconset_path = icns_path.replace('.icns', '.iconset')
    os.makedirs(iconset_path, exist_ok=True)
    
    # Define required sizes for icns
    sizes = [
        (16, 'icon_16x16.png'),
        (32, 'icon_16x16@2x.png'),
        (32, 'icon_32x32.png'),
        (64, 'icon_32x32@2x.png'),
        (128, 'icon_128x128.png'),
        (256, 'icon_128x128@2x.png'),
        (256, 'icon_256x256.png'),
        (512, 'icon_256x256@2x.png'),
        (512, 'icon_512x512.png'),
        (1024, 'icon_512x512@2x.png'),
    ]
    
    # Convert to required sizes
    for size, filename in sizes:
        output_path = os.path.join(iconset_path, filename)
        convert_svg_to_png(svg_file, output_path, size, size)
    
    # Convert iconset to icns using iconutil
    subprocess.run(['iconutil', '-c', 'icns', iconset_path, '-o', icns_path])
    
    # Clean up iconset
    subprocess.run(['rm', '-rf', iconset_path])
    print(f"Created {icns_path}")
    return True

# Main conversion
if __name__ == "__main__":
    # Parse command line arguments
    parser = argparse.ArgumentParser(description='Convert SVG icon to various formats')
    parser.add_argument('svg_path', nargs='?', default='gate-icon.svg',
                        help='Path to the SVG file to convert (default: gate-icon.svg)')
    args = parser.parse_args()
    
    # Change to icons directory
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    
    svg_file = args.svg_path
    
    # Convert to required PNG sizes
    sizes = [
        (32, '32x32.png'),
        (128, '128x128.png'),
        (256, '128x128@2x.png'),
    ]
    
    png_files = []
    for size, filename in sizes:
        if convert_svg_to_png(svg_file, filename, size, size):
            png_files.append((size, filename))
    
    # Create .ico file for Windows
    if png_files:
        # Add more sizes for better .ico
        extra_sizes = [(16, '16x16.png'), (48, '48x48.png'), (256, '256x256.png')]
        for size, filename in extra_sizes:
            if not os.path.exists(filename):
                convert_svg_to_png(svg_file, filename, size, size)
                png_files.append((size, filename))
        
        create_ico_file(png_files, 'icon.ico')
        
        # Clean up extra files
        for _, filename in extra_sizes:
            if os.path.exists(filename):
                os.remove(filename)
    
    # Create .icns file for macOS
    create_icns_file(svg_file, 'icon.icns')