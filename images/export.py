import os
# If you have multiple images open, you may need to adjust
img = gimp.image_list()[0]

savefn = gimp.pdb['file-png-save-defaults']
outpath = "C:\Users\Steven\Documents\programming\rust\omclone\images"

for lay in img.layers:
    # Even if your layer names contain spaces, not a problem
    if lay.name != "Bonds":
        outname = lay.name + ".png"
        savefn(img, lay, outname, outname)
    